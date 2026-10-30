//! Chạy pipeline dịch trên JSONL (stdin → stdout) cho eval / benchmark / redteam.
//!
//! CHỈ dùng với dữ liệu tổng hợp trong `eval/`. Cờ `--audit` in cả prompt gửi
//! provider ra stdout để kiểm tra; không bao giờ dùng với tin thật.
//!
//! Dòng vào:  {"id": "...", "request": {<translator::Request>},
//!             "history": [{"sender_id": "...", "lang": "vi", "text": "..."}]}
//! Dòng ra:   {"id": "...", "response": {...}, "streamed": {...},
//!             "audit": {"prompts": [{"prefix": "...", "body": "..."}]}}
//!
//! `history` (tuỳ chọn) được nạp vào bộ nhớ luồng trước khi dịch, như thể các tin
//! đó đã được gửi trước trong cùng luồng.
//!
//! Với `--judge`, mỗi dòng là một bản dịch cần chấm, gửi qua model-router (tầng mạnh):
//! Dòng vào:  {"id": "...", "judge": {<prompts::judge::JudgeInput>}}
//! Dòng ra:   {"id": "...", "verdict": {...}, "tier": "strong"} hoặc {"id": "...", "error": "..."}

use glossary::Store;
use model_router::{
    BoxFuture, Completion, Config, MockProvider, ModelRequest, OnDelta, OpenAiCompat, Provider,
    ProviderConfig, ProviderError, Registry, Router, Tier,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use telemetry::{JsonSink, Nop, Sink};
use translator::{Request, Response, Secrets, StreamEvent, Translator};

const USAGE: &str = "dùng: xt-cli [--audit] [--judge] [--telemetry FILE] [--glossary-dir DIR] [--config FILE] < cases.jsonl";

struct Args {
    audit: bool,
    judge: bool,
    telemetry: Option<PathBuf>,
    glossary_dir: PathBuf,
    config: PathBuf,
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args {
        audit: false,
        judge: false,
        telemetry: None,
        glossary_dir: "glossary/data".into(),
        config: "config/models.toml".into(),
    };
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut value = || it.next().ok_or_else(|| format!("thiếu giá trị cho {flag}"));
        match flag.as_str() {
            "--audit" => a.audit = true,
            "--judge" => a.judge = true,
            "--telemetry" => a.telemetry = Some(value()?.into()),
            "--glossary-dir" => a.glossary_dir = value()?.into(),
            "--config" => a.config = value()?.into(),
            _ => return Err(USAGE.into()),
        }
    }
    Ok(a)
}

#[derive(Deserialize)]
struct HistoryLine {
    sender_id: String,
    lang: String,
    text: String,
}

#[derive(Deserialize)]
struct Line {
    id: String,
    request: Request,
    #[serde(default)]
    history: Vec<HistoryLine>,
}

#[derive(Deserialize)]
struct JudgeLine {
    id: String,
    judge: prompts::judge::JudgeInput,
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

#[derive(Serialize)]
struct JudgeOut<'a> {
    id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    verdict: Option<prompts::judge::Verdict>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tier: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<&'static str>,
}

fn fail(msg: &str) -> ! {
    eprintln!("xt-cli: {msg}");
    std::process::exit(2)
}

type PromptLog = Arc<Mutex<Vec<Prompt>>>;

/// Bọc một provider để ghi lại prompt (chế độ `--audit`), với mọi loại provider.
struct Recording {
    inner: Arc<dyn Provider>,
    log: PromptLog,
}

impl Provider for Recording {
    fn complete<'a>(
        &'a self,
        model: &'a str,
        req: &'a ModelRequest,
        on_delta: OnDelta<'a>,
    ) -> BoxFuture<'a, Result<Completion, ProviderError>> {
        if let Ok(mut l) = self.log.lock() {
            l.push(Prompt {
                prefix: req.prefix.clone(),
                body: req.body.clone(),
            });
        }
        self.inner.complete(model, req, on_delta)
    }
}

fn registry(audit: Option<PromptLog>) -> Registry {
    let mock: Arc<dyn Provider> = Arc::new(MockProvider::with_fn(|_, req| {
        translator::mock::glossary_translate(req)
    }));
    let wrap = move |inner: Arc<dyn Provider>| -> Arc<dyn Provider> {
        match &audit {
            Some(log) => Arc::new(Recording {
                inner,
                log: log.clone(),
            }),
            None => inner,
        }
    };
    let wrap2 = wrap.clone();
    let mut r = Registry::default();
    r.register("mock", Box::new(move |_| wrap(mock.clone())));
    // Mô hình thật (eval chất lượng): --config trỏ tới file cấu hình local.
    r.register(
        "openai_compatible",
        Box::new(move |pc: &ProviderConfig| wrap2(Arc::new(OpenAiCompat::from_config(pc)))),
    );
    r
}

fn print_line(json: &str) {
    let stdout = std::io::stdout();
    let mut lock = stdout.lock();
    let _ = writeln!(lock, "{json}");
}

/// Đọc từng dòng JSONL khác rỗng; không in nội dung dòng lỗi, chỉ số dòng.
fn read_lines<T: for<'de> Deserialize<'de>>() -> impl Iterator<Item = T> {
    std::io::stdin()
        .lock()
        .lines()
        .enumerate()
        .filter_map(|(n, line)| {
            let line = line.unwrap_or_else(|e| fail(&e.to_string()));
            if line.trim().is_empty() {
                return None;
            }
            Some(
                serde_json::from_str(&line)
                    .unwrap_or_else(|_| fail(&format!("dòng {} sai định dạng", n + 1))),
            )
        })
        .collect::<Vec<T>>()
        .into_iter()
}

async fn judge(router: &Router) {
    for input in read_lines::<JudgeLine>() {
        let req = prompts::judge::build(&input.judge);
        let out = match router.complete(Tier::Strong, &req, false, &|_| {}).await {
            Ok(routed) => match prompts::judge::parse(&routed.completion.text) {
                Some(v) => JudgeOut {
                    id: &input.id,
                    verdict: Some(v),
                    tier: Some(routed.tier.as_str()),
                    error: None,
                },
                None => JudgeOut {
                    id: &input.id,
                    verdict: None,
                    tier: Some(routed.tier.as_str()),
                    error: Some("judge_unparsable"),
                },
            },
            Err(e) => JudgeOut {
                id: &input.id,
                verdict: None,
                tier: None,
                error: Some(e.code()),
            },
        };
        print_line(&serde_json::to_string(&out).unwrap_or_else(|e| fail(&e.to_string())));
    }
}

#[tokio::main]
async fn main() {
    let args = parse_args().unwrap_or_else(|e| fail(&e));
    let raw = std::fs::read_to_string(&args.config).unwrap_or_else(|e| fail(&e.to_string()));
    let cfg = Config::parse(&raw).unwrap_or_else(|e| fail(&e.to_string()));

    let prompts_log: PromptLog = Arc::default();
    let registry = registry(args.audit.then(|| prompts_log.clone()));
    let router = Router::new(&cfg, &registry).unwrap_or_else(|e| fail(&e.to_string()));
    if args.judge {
        judge(&router).await;
        return;
    }

    let store = Store::load_dir(&args.glossary_dir).unwrap_or_else(|e| fail(&e.to_string()));
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

    for input in read_lines::<Line>() {
        for h in &input.history {
            t.memory()
                .record(&input.request.thread_id, &h.sender_id, &h.lang, &h.text);
        }
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
        print_line(&serde_json::to_string(&out).unwrap_or_else(|e| fail(&e.to_string())));
    }
}
