//! The HTTP window onto `Live` (contract: docs/api.md). Every handler reads the engine's
//! own state or calls one `Live` method; the server keeps no state of its own beyond the
//! per-client stream position. Serving allocates in here (JSON frames, tail copies),
//! never on the engine's per-event path.

use std::collections::VecDeque;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering::Relaxed;
use std::time::Duration;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use futures_util::stream::Stream;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::engine::{Live, TracebackError};
use crate::pending::{Pending, ReviewError};
use crate::tail::TailFrame;

#[derive(Clone)]
struct App {
    live: Arc<Live>,
    ui_dir: Option<PathBuf>,
    listen: SocketAddr,
}

pub struct Server {
    pub addr: SocketAddr,
    rt: tokio::runtime::Runtime,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
}

impl Server {
    /// Binds and starts serving on its own small runtime; returns at once.
    pub fn start(live: Arc<Live>, listen: SocketAddr, ui_dir: Option<PathBuf>) -> anyhow::Result<Server> {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build()?;
        let listener = rt.block_on(tokio::net::TcpListener::bind(listen))?;
        let addr = listener.local_addr()?;
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let app = router(App { live, ui_dir, listen: addr });
        rt.spawn(async move {
            let _ = axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = rx.await;
                })
                .await;
        });
        Ok(Server { addr, rt, shutdown: Some(tx) })
    }

    /// Ctrl-C stops the engine; the CLI then shuts the server down after the report.
    pub fn install_ctrl_c(&self, live: Arc<Live>) {
        self.rt.spawn(async move {
            if tokio::signal::ctrl_c().await.is_ok() {
                eprintln!("ulpf: stopping");
                live.stop();
            }
        });
    }

    pub fn shutdown(mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        self.rt.shutdown_timeout(Duration::from_secs(2));
    }
}

fn router(app: App) -> axum::Router {
    axum::Router::new()
        .route("/", get(ui_index))
        .route("/app.js", get(ui_js))
        .route("/app.css", get(ui_css))
        .route("/api/status", get(status))
        .route("/api/metrics", get(metrics))
        .route("/api/tail", get(tail))
        .route("/api/stream", get(stream))
        .route("/api/parsers", get(parsers))
        .route("/api/parsers/reload", post(reload))
        .route("/api/pending", get(pending_list))
        .route("/api/pending/{id}", get(pending_get).put(pending_put))
        .route("/api/pending/{id}/regenerate", post(pending_regenerate))
        .route("/api/pending/{id}/approve", post(pending_approve))
        .route("/api/pending/{id}/reject", post(pending_reject))
        .route("/api/events/{raw_id}", get(traceback))
        .with_state(app)
}

// ---- errors -------------------------------------------------------------------------

struct ApiError {
    status: StatusCode,
    reason: &'static str,
    error: String,
    extra: Value,
}

impl ApiError {
    fn new(status: StatusCode, reason: &'static str, error: impl Into<String>) -> ApiError {
        ApiError { status, reason, error: error.into(), extra: json!({}) }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut body = json!({ "error": self.error, "reason": self.reason });
        if let (Some(dst), Some(src)) = (body.as_object_mut(), self.extra.as_object()) {
            for (k, v) in src {
                dst.insert(k.clone(), v.clone());
            }
        }
        (self.status, Json(body)).into_response()
    }
}

fn review_error(live: &Live, e: ReviewError) -> ApiError {
    let err = match e {
        ReviewError::NotFound(id) => ApiError::new(StatusCode::NOT_FOUND, "not_found", format!("no pending proposal `{id}`")),
        ReviewError::Invalid(problems) => ApiError { status: StatusCode::UNPROCESSABLE_ENTITY, reason: "invalid", error: format!("definition does not load: {}", problems.join("; ")), extra: json!({ "problems": problems }) },
        ReviewError::Conflict(name) => ApiError::new(StatusCode::CONFLICT, "conflict", format!("an active parser is already named `{name}`; change [parser] name first")),
        ReviewError::Io(msg) => ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "io", msg),
    };
    if err.status.is_client_error() {
        live.review_errors.fetch_add(1, Relaxed);
    }
    err
}

// ---- frames -------------------------------------------------------------------------

fn tail_json(frame: TailFrame) -> Value {
    json!({
        "events": frame.events.iter().map(|(id, line)| json!({ "raw_id": id, "line": serde_json::from_slice::<Value>(line).unwrap_or(Value::Null) })).collect::<Vec<_>>(),
        "skipped": frame.skipped,
        "latest_raw_id": frame.latest,
    })
}

fn parsers_json(live: &Live) -> Value {
    let pipeline = live.pipeline();
    let hits = live.parser_hits.lock().unwrap_or_else(|e| e.into_inner()).clone();
    Value::Array(
        pipeline
            .registry
            .iter()
            .map(|p| {
                let d = p.definition();
                json!({
                    "name": d.parser.name,
                    "vendor": d.parser.vendor,
                    "product": d.parser.product,
                    "priority": d.matcher.priority,
                    "strategy": d.strategy.kind.name(),
                    "subs": d.sub.len(),
                    "origin": if d.parser.description.as_deref().is_some_and(|s| s.starts_with("Inferred from")) { "approved" } else { "hand" },
                    "detected": hits.get(&d.parser.name).copied().unwrap_or(0),
                })
            })
            .collect(),
    )
}

fn metrics_frame(live: &Live) -> Value {
    let buffered = live.inference.buffered();
    let pending_ids: Vec<String> = live.pending.as_ref().map(Pending::list).unwrap_or_default().into_iter().map(|p| p.id).collect();
    let sources: Vec<Value> = live
        .sources
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .map(|(name, s)| {
            let id = Pending::id_for(name);
            let mut last = String::new();
            if s.last_seen_nanos > 0 {
                ulpf_time::format_rfc3339(s.last_seen_nanos, &mut last);
            }
            json!({
                "name": name, "events": s.events, "detected": s.detected, "no_parser": s.no_parser,
                "buffered": buffered.get(name).copied().unwrap_or(0),
                "last_seen": if last.is_empty() { Value::Null } else { Value::String(last) },
                "pending_id": if pending_ids.contains(&id) { Value::String(id) } else { Value::Null },
            })
        })
        .collect();
    json!({
        "engine": live.snapshot(),
        "sources": sources,
        "parsers": parsers_json(live),
        "pending_generation": live.pending_generation.load(Relaxed),
        "server": {
            "sse_clients": live.sse_clients.load(Relaxed),
            "review_errors": live.review_errors.load(Relaxed),
            "uptime_secs": live.started.elapsed().as_secs_f64(),
        },
    })
}

// ---- handlers -----------------------------------------------------------------------

async fn status(State(app): State<App>) -> Json<Value> {
    let live = &app.live;
    let mut started = String::new();
    ulpf_time::format_rfc3339(live.started_nanos, &mut started);
    Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "started_at": started,
        "listen": app.listen.to_string(),
        "store": live.store_dir,
        "parsers_dir": live.parsers_dir,
        "pending_dir": live.pending.as_ref().map(|p| p.dir().to_path_buf()),
        "output": live.output,
        "watch": live.watch,
        "threads": live.threads,
        "queue_capacity": live.queue_cap,
        "tail_capacity": live.tail.capacity(),
        "infer_threshold": live.inference.threshold,
    }))
}

async fn metrics(State(app): State<App>) -> Json<Value> {
    Json(metrics_frame(&app.live))
}

#[derive(Deserialize)]
struct TailQuery {
    after: Option<u64>,
    limit: Option<usize>,
}

async fn tail(State(app): State<App>, Query(q): Query<TailQuery>) -> Json<Value> {
    Json(tail_json(app.live.tail.since(q.after, q.limit.unwrap_or(100).clamp(1, 500))))
}

async fn parsers(State(app): State<App>) -> Json<Value> {
    Json(parsers_json(&app.live))
}

async fn reload(State(app): State<App>) -> Json<Value> {
    Json(serde_json::to_value(app.live.reload_parsers()).unwrap_or(Value::Null))
}

async fn pending_list(State(app): State<App>) -> Json<Value> {
    let list = app.live.pending.as_ref().map(Pending::list).unwrap_or_default();
    Json(Value::Array(
        list.into_iter()
            .map(|p| {
                let mut created = String::new();
                ulpf_time::format_rfc3339(p.created_nanos, &mut created);
                json!({ "id": p.id, "source": p.source, "name": p.name, "created": created, "lines": p.lines, "templates": p.templates, "unmatched": p.unmatched, "edited": p.edited, "problems": p.problems })
            })
            .collect(),
    ))
}

fn pending_of(live: &Live) -> Result<&Pending, ApiError> {
    live.pending.as_ref().ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "not_found", "inference is disabled: no pending directory"))
}

async fn pending_get(State(app): State<App>, Path(id): Path<String>) -> Result<Json<Value>, ApiError> {
    let pending = pending_of(&app.live)?;
    let d = pending.get(&id).map_err(|e| review_error(&app.live, e))?;
    Ok(Json(json!({ "id": d.id, "source": d.source, "definition": d.definition, "problems": d.problems, "evidence": d.record.evidence, "edited": d.record.edited })))
}

#[derive(Deserialize)]
struct PutBody {
    definition: String,
}

async fn pending_put(State(app): State<App>, Path(id): Path<String>, Json(body): Json<PutBody>) -> Result<Json<Value>, ApiError> {
    let problems = app.live.put_text(&id, &body.definition).map_err(|e| review_error(&app.live, e))?;
    Ok(Json(json!({ "problems": problems })))
}

#[derive(Deserialize)]
struct RegenerateBody {
    #[serde(default)]
    keep: Vec<u64>,
    #[serde(default)]
    merge: Vec<Vec<u64>>,
}

async fn pending_regenerate(State(app): State<App>, Path(id): Path<String>, Json(body): Json<RegenerateBody>) -> Result<Json<Value>, ApiError> {
    let (definition, problems) = app.live.regenerate(&id, &body.keep, &body.merge).map_err(|e| review_error(&app.live, e))?;
    Ok(Json(json!({ "definition": definition, "problems": problems })))
}

async fn pending_approve(State(app): State<App>, Path(id): Path<String>) -> Result<Json<Value>, ApiError> {
    let report = app.live.approve(&id).map_err(|e| review_error(&app.live, e))?;
    Ok(Json(serde_json::to_value(report).unwrap_or(Value::Null)))
}

async fn pending_reject(State(app): State<App>, Path(id): Path<String>) -> Result<Json<Value>, ApiError> {
    let moved = app.live.reject(&id).map_err(|e| review_error(&app.live, e))?;
    Ok(Json(json!({ "id": id, "moved_to": moved })))
}

async fn traceback(State(app): State<App>, Path(raw_id): Path<u64>) -> Result<Json<Value>, ApiError> {
    match app.live.traceback(raw_id) {
        Ok(t) => Ok(Json(serde_json::to_value(t).unwrap_or(Value::Null))),
        Err(TracebackError::NotFound { store_len }) => Err(ApiError { status: StatusCode::NOT_FOUND, reason: "not_found", error: format!("raw id {raw_id} was never issued (store holds {store_len})"), extra: json!({ "store_len": store_len }) }),
        Err(TracebackError::Io(e)) => Err(ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "io", e)),
    }
}

// ---- stream -------------------------------------------------------------------------

#[derive(Deserialize)]
struct StreamQuery {
    tail: Option<usize>,
}

/// Drops with the stream: the client count stays exact across disconnects.
struct ClientGuard(Arc<Live>);

impl Drop for ClientGuard {
    fn drop(&mut self) {
        self.0.sse_clients.fetch_sub(1, Relaxed);
    }
}

struct StreamState {
    guard: ClientGuard,
    queue: VecDeque<Event>,
    last_id: Option<u64>,
    pending_generation: u64,
    tick: u64,
    initial: usize,
}

const TICK: Duration = Duration::from_millis(250);
const TAIL_PER_TICK: usize = 200;

async fn stream(State(app): State<App>, Query(q): Query<StreamQuery>) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    app.live.sse_clients.fetch_add(1, Relaxed);
    let state = StreamState { guard: ClientGuard(Arc::clone(&app.live)), queue: VecDeque::new(), last_id: None, pending_generation: app.live.pending_generation.load(Relaxed), tick: 0, initial: q.tail.unwrap_or(100).clamp(1, 500) };
    let stream = futures_util::stream::unfold(state, |mut st| async move {
        loop {
            if let Some(ev) = st.queue.pop_front() {
                return Some((Ok::<Event, Infallible>(ev), st));
            }
            let live = &st.guard.0;
            if st.tick == 0 {
                let frame = live.tail.since(None, st.initial);
                st.last_id = frame.latest;
                let count = live.pending.as_ref().map(|p| p.list().len()).unwrap_or(0);
                let hello = json!({ "latest_raw_id": frame.latest, "pending_generation": st.pending_generation, "pending_count": count, "tail": tail_json(frame) });
                st.queue.push_back(event("hello", &hello));
                st.queue.push_back(event("metrics", &metrics_frame(live)));
                st.tick += 1;
                continue;
            }
            tokio::time::sleep(TICK).await;
            st.tick += 1;
            let frame = live.tail.since(st.last_id, TAIL_PER_TICK);
            if !frame.events.is_empty() {
                st.last_id = frame.latest;
                st.queue.push_back(event("tail", &tail_json(frame)));
            }
            if st.tick % 2 == 0 {
                st.queue.push_back(event("metrics", &metrics_frame(live)));
            }
            let generation = live.pending_generation.load(Relaxed);
            if generation != st.pending_generation {
                st.pending_generation = generation;
                let count = live.pending.as_ref().map(|p| p.list().len()).unwrap_or(0);
                st.queue.push_back(event("pending", &json!({ "generation": generation, "count": count })));
            }
        }
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}

fn event(kind: &str, data: &Value) -> Event {
    Event::default().event(kind).data(data.to_string())
}

// ---- UI -----------------------------------------------------------------------------

const INDEX_HTML: &str = include_str!("../../../ui/dist/index.html");
const APP_JS: &str = include_str!("../../../ui/dist/app.js");
const APP_CSS: &str = include_str!("../../../ui/dist/app.css");

fn asset(app: &App, name: &str, embedded: &'static str, mime: &'static str) -> Response {
    let body: Vec<u8> = match &app.ui_dir {
        Some(dir) => match std::fs::read(dir.join(name)) {
            Ok(b) => b,
            Err(e) => return (StatusCode::NOT_FOUND, format!("{}: {e}", dir.join(name).display())).into_response(),
        },
        None => embedded.as_bytes().to_vec(),
    };
    ([(header::CONTENT_TYPE, mime), (header::CACHE_CONTROL, "no-cache")], body).into_response()
}

async fn ui_index(State(app): State<App>) -> Response {
    asset(&app, "index.html", INDEX_HTML, "text/html; charset=utf-8")
}
async fn ui_js(State(app): State<App>) -> Response {
    asset(&app, "app.js", APP_JS, "text/javascript; charset=utf-8")
}
async fn ui_css(State(app): State<App>) -> Response {
    asset(&app, "app.css", APP_CSS, "text/css; charset=utf-8")
}
