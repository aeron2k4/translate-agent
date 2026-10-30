//! Chạy pipeline dịch trên JSONL (stdin → stdout) cho eval / redteam.
//!
//! CHỈ dùng với dữ liệu tổng hợp trong `eval/`. Cờ `--audit` in cả prompt gửi
//! provider ra stdout để red team kiểm tra; không bao giờ dùng với tin thật.
//!
//! Dòng vào:  {"id": "...", "request": {<translator::Request>}}
//! Dòng ra:   {"id": "...", "response": {...}, "streamed": {...},
//!             "audit": {"prompts": [{"prefix": "...", "body": "..."}]}}

use glossary::Store;
use model_router::{Config, MockProvider, Registry, Router};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use telemetry::{JsonSink, Nop, Sink};
use translator::{Request, Response, Secrets, StreamEvent, Translator};

const USAGE: &str =
    "dùng: xt-cli [--audit] [--telemetry FILE] [--glossary-dir DIR] [--config FILE] < cases.jsonl";

struct Args {
    audit: bool,
    telemetry: Option<PathBuf>,
    glossary_dir: PathBuf,
    config: PathBuf,
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args {
        audit: false,
        telemetry: None,
        glossary_dir: "glossary/data".into(),
        config: "config/models.toml".into(),
    };
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut value = || it.next().ok_or_else(|| format!("thiếu giá trị cho {flag}"));
        match flag.as_str() {
            "--audit" => a.audit = true,
            "--telemetry" => a.telemetry = Some(value()?.into()),
            "--glossary-dir" => a.glossary_dir = value()?.into(),
            "--config" => a.config = value()?.into(),
            _ => return Err(USAGE.into()),
        }
    }
    Ok(a)
}

#[derive(Deserialize)]
struct Line {
    id: String,
    request: Request,
}

#[derive(Serialize)]
struct Prompt {
    prefix: String,
    body: String,
}

#[derive(Serialize)]
struct Audit {
    prompts: Vec<Prompt>,
}

#[derive(Serialize)]
struct Out<'a> {
    id: &'a str,
    response: &'a Response,
    /// Nội dung client nhận được qua stream, theo ngôn ngữ.
    streamed: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    audit: Option<Audit>,
}

fn fail(msg: &str) -> ! {
    eprintln!("xt-cli: {msg}");
    std::process::exit(2)
}

#[tokio::main]
async fn main() {
    let args = parse_args().unwrap_or_else(|e| fail(&e));
    let store = Store::load_dir(&args.glossary_dir).unwrap_or_else(|e| fail(&e.to_string()));
    let raw = std::fs::read_to_string(&args.config).unwrap_or_else(|e| fail(&e.to_string()));
    let cfg = Config::parse(&raw).unwrap_or_else(|e| fail(&e.to_string()));

    let prompts_log: Arc<Mutex<Vec<Prompt>>> = Arc::default();
    let log = prompts_log.clone();
    let audit = args.audit;
    let provider = Arc::new(MockProvider::with_fn(move |_, req| {
        if audit && let Ok(mut l) = log.lock() {
            l.push(Prompt {
                prefix: req.prefix.clone(),
                body: req.body.clone(),
            });
        }
        translator::mock::glossary_translate(req)
    }));
    let mut registry = Registry::default();
    registry.register("mock", Box::new(move |_| provider.clone()));
    let router = Router::new(&cfg, &registry).unwrap_or_else(|e| fail(&e.to_string()));

    let sink: Arc<dyn Sink> = match &args.telemetry {
        Some(p) => Arc::new(JsonSink::new(
            std::fs::File::create(p).unwrap_or_else(|e| fail(&e.to_string())),
        )),
        None => Arc::new(Nop),
    };
    let secrets = Secrets {
        cache_key: b"xt-cli-eval-only".to_vec(),
        id_hash_key: b"xt-cli-eval-only".to_vec(),
    };
    let t = Translator::new(Arc::new(store), Arc::new(router), sink, &secrets);

    let stdout = std::io::stdout();
    for (n, line) in std::io::stdin().lock().lines().enumerate() {
        let line = line.unwrap_or_else(|e| fail(&e.to_string()));
        if line.trim().is_empty() {
            continue;
        }
        // Không in nội dung dòng lỗi, chỉ số dòng.
        let input: Line = serde_json::from_str(&line)
            .unwrap_or_else(|_| fail(&format!("dòng {} sai định dạng", n + 1)));
        let streamed: Mutex<BTreeMap<String, String>> = Mutex::default();
        let on = |e: StreamEvent<'_>| {
            if let Ok(mut s) = streamed.lock() {
                match e {
                    StreamEvent::Delta { lang, text } => {
                        s.entry(lang.to_owned()).or_default().push_str(text)
                    }
                    StreamEvent::Reset { lang } => {
                        s.remove(lang);
                    }
                }
            }
        };
        let response = t.translate(&input.request, &on).await;
        let audit = args.audit.then(|| Audit {
            prompts: prompts_log
                .lock()
                .map(|mut l| std::mem::take(&mut *l))
                .unwrap_or_default(),
        });
        let out = Out {
            id: &input.id,
            response: &response,
            streamed: streamed.into_inner().unwrap_or_default(),
            audit,
        };
        let json = serde_json::to_string(&out).unwrap_or_else(|e| fail(&e.to_string()));
        let mut lock = stdout.lock();
        let _ = writeln!(lock, "{json}");
    }
}
