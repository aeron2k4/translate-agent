//! make dev: chạy local toàn bộ pipeline qua HTTP, không TEE, dùng mock enclave
//! và provider giả. Không log nội dung request/response.
//!
//! - GET  /healthz
//! - GET  /v1/attestation
//! - POST /v1/translate            (JSON → JSON)
//! - POST /v1/translate/stream     (JSON → NDJSON: delta… rồi done)
//! - POST /v1/corrections          (bản sửa HITL; đề xuất đơn vị ghi vào hàng chờ review)
//! - GET  /v1/threads/{thread}/messages/{message}/corrected?lang=en
//! - POST /v1/threads/{thread}/close
//! - POST /v1/threads/{thread}/members/{member}/leave

mod enclave;

use axum::body::{Body, Bytes};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use enclave::{Enclave, EnclaveError, MockEnclave, Sealed};
use model_router::{Config, Registry};
use serde::Deserialize;
use serde_json::json;
use std::convert::Infallible;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio_stream::StreamExt;
use tokio_stream::wrappers::UnboundedReceiverStream;
use translator::hitl::Correction;
use translator::{Request, Secrets, StreamEvent, Translator};

struct AppState {
    translator: Translator,
    enclave: Box<dyn Enclave>,
    /// Đề xuất cho đơn vị (chỉ cặp thuật ngữ + ví dụ đã ẩn danh), JSONL cho
    /// `python -m xt_admin.review <queue.json> ingest`.
    suggestions: Mutex<Option<std::fs::File>>,
}

type Shared = Arc<AppState>;

#[derive(Deserialize)]
struct TranslateBody {
    #[serde(flatten)]
    request: Request,
    /// Tin đã mã hoá bằng khoá phiên; có thì thay cho `text`.
    #[serde(default)]
    sealed: Option<Sealed>,
}

fn error(status: StatusCode, code: &'static str) -> Response {
    (status, Json(json!({ "error": code }))).into_response()
}

fn open_request(state: &AppState, body: TranslateBody) -> Result<Request, EnclaveError> {
    let mut req = body.request;
    if let Some(sealed) = &body.sealed {
        req.text = state.enclave.unseal(sealed)?;
    }
    Ok(req)
}

async fn translate(State(state): State<Shared>, Json(body): Json<TranslateBody>) -> Response {
    let req = match open_request(&state, body) {
        Ok(r) => r,
        Err(e) => return error(StatusCode::BAD_REQUEST, e.code()),
    };
    Json(state.translator.translate(&req, &|_| {}).await).into_response()
}

async fn translate_stream(
    State(state): State<Shared>,
    Json(body): Json<TranslateBody>,
) -> Response {
    let req = match open_request(&state, body) {
        Ok(r) => r,
        Err(e) => return error(StatusCode::BAD_REQUEST, e.code()),
    };
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<serde_json::Value>();
    tokio::spawn(async move {
        let delta_tx = tx.clone();
        let on = move |e: StreamEvent<'_>| {
            let _ = delta_tx.send(match e {
                StreamEvent::Delta { lang, text } => {
                    json!({"type": "delta", "lang": lang, "text": text})
                }
                StreamEvent::Reset { lang } => json!({"type": "reset", "lang": lang}),
            });
        };
        let response = state.translator.translate(&req, &on).await;
        let _ = tx.send(json!({"type": "done", "response": response}));
    });
    let lines = UnboundedReceiverStream::new(rx)
        .map(|v| Ok::<_, Infallible>(Bytes::from(format!("{v}\n"))));
    Response::builder()
        .header("content-type", "application/x-ndjson")
        .body(Body::from_stream(lines))
        .unwrap_or_else(|_| error(StatusCode::INTERNAL_SERVER_ERROR, "stream_failed"))
}

async fn corrections(State(state): State<Shared>, Json(c): Json<Correction>) -> Response {
    let outcome = state.translator.submit_correction(&c);
    if let Some(s) = &outcome.suggestion
        && let Ok(mut guard) = state.suggestions.lock()
        && let Some(file) = guard.as_mut()
        && let Ok(line) = serde_json::to_string(s)
    {
        let _ = writeln!(file, "{line}");
    }
    Json(json!({ "scope": outcome.scope, "stored": outcome.stored, "suggested": outcome.suggestion.is_some() }))
        .into_response()
}

#[derive(Deserialize)]
struct LangQuery {
    lang: String,
}

async fn corrected(
    State(state): State<Shared>,
    Path((thread, message)): Path<(String, String)>,
    Query(q): Query<LangQuery>,
) -> Response {
    match state
        .translator
        .corrected_translation(&thread, &message, &q.lang)
    {
        Some(t) => Json(t).into_response(),
        None => error(StatusCode::NOT_FOUND, "no_correction"),
    }
}

async fn close(State(state): State<Shared>, Path(thread): Path<String>) -> StatusCode {
    state.translator.close_session(&thread);
    StatusCode::NO_CONTENT
}

async fn leave(
    State(state): State<Shared>,
    Path((thread, member)): Path<(String, String)>,
) -> StatusCode {
    state.translator.member_left(&thread, &member);
    StatusCode::NO_CONTENT
}

fn app(state: Shared) -> Router {
    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route(
            "/v1/attestation",
            get(|State(s): State<Shared>| async move { Json(s.enclave.attestation()) }),
        )
        .route("/v1/translate", post(translate))
        .route("/v1/translate/stream", post(translate_stream))
        .route("/v1/corrections", post(corrections))
        .route(
            "/v1/threads/{thread}/messages/{message}/corrected",
            get(corrected),
        )
        .route("/v1/threads/{thread}/close", post(close))
        .route("/v1/threads/{thread}/members/{member}/leave", post(leave))
        .with_state(state)
}

fn build_state(repo: &std::path::Path, suggestions: Option<PathBuf>) -> Result<AppState, String> {
    let store =
        glossary::Store::load_dir(&repo.join("glossary/data")).map_err(|e| e.to_string())?;
    let raw =
        std::fs::read_to_string(repo.join("config/models.toml")).map_err(|e| e.to_string())?;
    let cfg = Config::parse(&raw).map_err(|e| e.to_string())?;
    let provider = Arc::new(translator::mock::glossary_mock());
    let mut registry = Registry::default();
    registry.register("mock", Box::new(move |_| provider.clone()));
    let router = model_router::Router::new(&cfg, &registry).map_err(|e| e.to_string())?;
    let secrets = Secrets {
        cache_key: b"devserver-only".to_vec(),
        id_hash_key: b"devserver-only".to_vec(),
    };
    let sink = Arc::new(telemetry::JsonSink::new(std::io::stderr()));
    let suggestions = match suggestions {
        Some(p) => {
            if let Some(dir) = p.parent() {
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            Some(
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(p)
                    .map_err(|e| e.to_string())?,
            )
        }
        None => None,
    };
    Ok(AppState {
        translator: Translator::new(Arc::new(store), Arc::new(router), sink, &secrets),
        enclave: Box::new(MockEnclave::new(*b"xt-dev-only-key-not-for-prod!!!!")),
        suggestions: Mutex::new(suggestions),
    })
}

#[tokio::main]
async fn main() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let addr = std::env::var("XT_DEV_ADDR").unwrap_or_else(|_| "127.0.0.1:8787".into());
    let state =
        build_state(&repo, Some(repo.join("var/review/suggestions.jsonl"))).unwrap_or_else(|e| {
            eprintln!("devserver: {e}");
            std::process::exit(2)
        });
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| {
            eprintln!("devserver: không bind được {addr}: {e}");
            std::process::exit(2)
        });
    eprintln!("devserver (mock enclave, provider giả) đang chạy ở http://{addr}");
    if let Err(e) = axum::serve(listener, app(Arc::new(state))).await {
        eprintln!("devserver: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request as HttpRequest;
    use tower::ServiceExt;

    fn test_app() -> (Router, Shared) {
        let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let state = Arc::new(build_state(&repo, None).unwrap());
        (app(state.clone()), state)
    }

    async fn post_json(app: Router, uri: &str, body: serde_json::Value) -> (StatusCode, String) {
        let res = app
            .oneshot(
                HttpRequest::post(uri)
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = res.status();
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, String::from_utf8(bytes.to_vec()).unwrap())
    }

    #[tokio::test]
    async fn translates_sealed_messages() {
        let (app, _) = test_app();
        let enclave = MockEnclave::new(*b"xt-dev-only-key-not-for-prod!!!!");
        let sealed = enclave.seal("đặt cọc 5 củ để lái thử").unwrap();
        let (status, body) = post_json(
            app,
            "/v1/translate",
            json!({"thread_id": "t", "sender_id": "s", "text": "", "target_langs": ["en"], "sealed": sealed}),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        let text = v["translations"][0]["text"].as_str().unwrap();
        assert!(
            text.contains("5 million VND") && text.contains("test drive"),
            "{text}"
        );
    }

    #[tokio::test]
    async fn rejects_bad_seal_without_echoing_content() {
        let (app, _) = test_app();
        let sealed = json!({"key_id": "dev", "nonce": "00", "ciphertext": "zz"});
        let (status, body) = post_json(
            app,
            "/v1/translate",
            json!({"thread_id": "t", "sender_id": "s", "text": "", "target_langs": ["en"], "sealed": sealed}),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body, r#"{"error":"enclave_malformed"}"#);
    }

    #[tokio::test]
    async fn streams_ndjson_ending_with_done() {
        let (app, _) = test_app();
        let (status, body) = post_json(
            app,
            "/v1/translate/stream",
            json!({"thread_id": "t", "sender_id": "s", "text": "em muốn lái thử", "target_langs": ["en"]}),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let lines: Vec<serde_json::Value> = body
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(lines.last().unwrap()["type"], "done");
        assert!(lines.iter().any(|l| l["type"] == "delta"));
    }

    #[tokio::test]
    async fn group_correction_then_close_session() {
        let (app, _) = test_app();
        let (status, _) = post_json(
            app.clone(),
            "/v1/corrections",
            json!({
                "thread_id": "t9", "message_id": "m1", "user_id": "u", "scope": "group",
                "domain": "automotive", "source_lang": "vi", "target_lang": "en",
                "corrected_text": "I want a test ride"
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let get = |app: Router| async move {
            app.oneshot(
                HttpRequest::get("/v1/threads/t9/messages/m1/corrected?lang=en")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
            .status()
        };
        assert_eq!(get(app.clone()).await, StatusCode::OK);
        let (status, _) = post_json(app.clone(), "/v1/threads/t9/close", json!({})).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert_eq!(get(app).await, StatusCode::NOT_FOUND);
    }
}
