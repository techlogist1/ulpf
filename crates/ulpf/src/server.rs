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

use axum::body::{Body, Bytes};

use crate::engine::{DriftState, IntegrityError, Live, ReplayError, TracebackError, output_is_sink};
use crate::outfile::{Output, line_id};
use crate::pivot::{Order, PivotQuery};
use ulpf_normalize::EntityKind;
use crate::pending::{Pending, ReviewError};
use crate::tail::TailFrame;

#[derive(Clone)]
struct App {
    live: Arc<Live>,
    ui_dir: Option<PathBuf>,
    listen: SocketAddr,
    /// One metrics frame per tick however many clients ask: the hundredth SSE client
    /// costs a clone, not another walk of the pending directory.
    frame: Arc<std::sync::Mutex<Option<(std::time::Instant, Value)>>>,
}

const FRAME_TTL: Duration = Duration::from_millis(200);

fn cached_frame(app: &App) -> Value {
    {
        let slot = app.frame.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((at, v)) = slot.as_ref()
            && at.elapsed() < FRAME_TTL
        {
            return v.clone();
        }
    }
    // computed outside the lock: two clients racing at the boundary both compute, neither waits
    let v = metrics_frame(&app.live);
    *app.frame.lock().unwrap_or_else(|e| e.into_inner()) = Some((std::time::Instant::now(), v.clone()));
    v
}

pub struct Server {
    pub addr: SocketAddr,
    rt: tokio::runtime::Runtime,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
}

impl Server {
    /// Binds and starts serving on its own small runtime; returns at once.
    pub fn start(live: Arc<Live>, listen: SocketAddr, ui_dir: Option<PathBuf>) -> anyhow::Result<Server> {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(4).enable_all().build()?;
        let listener = rt.block_on(tokio::net::TcpListener::bind(listen))?;
        let addr = listener.local_addr()?;
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let app = router(App { live, ui_dir, listen: addr, frame: Arc::new(std::sync::Mutex::new(None)) });
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
        .route("/api/events/{raw_id}/bytes", get(traceback_bytes))
        .route("/api/export", get(export))
        .route("/api/replay", get(replay_get).post(replay_post))
        .route("/api/drift", get(drift))
        .route("/api/integrity", get(integrity))
        .route("/api/integrity/verify", post(integrity_verify))
        .route("/api/integrity/attestation", get(attestation))
        .route("/api/pivot", get(pivot))
        .route("/api/entities", get(entities))
        .route("/api/replay/{version}/diff", get(replay_diff))
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
                    "origin": if d.parser.origin.as_deref() == Some("inferred") { "approved" } else { "hand" },
                    "version": d.parser.version,
                    "detected": hits.get(&d.parser.name).copied().unwrap_or(0),
                })
            })
            .collect(),
    )
}

fn metrics_frame(live: &Live) -> Value {
    let buffered = live.inference.buffered();
    let pending_ids: Vec<String> = live.pending.as_ref().map(Pending::ids).unwrap_or_default();
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
                "parse_failed": s.parse_failed,
                "buffered": buffered.get(name).copied().unwrap_or(0),
                "last_seen": if last.is_empty() { Value::Null } else { Value::String(last) },
                "pending_id": if pending_ids.contains(&id) { Value::String(id) } else { Value::Null },
                "parser": s.established_parser(),
                "window_rate": s.window_rate,
                "baseline_rate": s.baseline_rate(),
                "drift": s.drift,
            })
        })
        .collect();
    json!({
        "engine": live.snapshot(),
        "queue": { "depth": live.in_flight.load(Relaxed).clamp(0, live.queue_cap as i64), "capacity": live.queue_cap },
        "rate": rate_json(live),
        "sources": sources,
        "parsers": parsers_json(live),
        "pending_generation": live.pending_generation.load(Relaxed),
        "replay": replay_summary(live),
        "integrity": live.integrity_summary(),
        "pivot": live.pivot_counters.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map(|c| json!({ "batches": c.batches.load(Relaxed), "postings": c.postings.load(Relaxed), "blocked": c.blocked.load(Relaxed), "errors": c.errors.load(Relaxed) })),
        "syslog": { "udp_datagrams": live.metrics.syslog_udp_datagrams.load(Relaxed), "tcp_events": live.metrics.syslog_tcp_events.load(Relaxed), "tcp_connections": live.metrics.syslog_tcp_connections.load(Relaxed) },
        "drift": live.drift_alerts().into_iter().filter(|a| matches!(a.state, DriftState::Tripped | DriftState::Proposed)).collect::<Vec<_>>(),
        "parquet": {
            "rows": live.metrics.parquet_rows.load(Relaxed),
            "files": live.metrics.parquet_files.load(Relaxed),
            "errors": live.metrics.parquet_errors.load(Relaxed),
        },
        "server": {
            "sse_clients": live.sse_clients.load(Relaxed),
            "review_errors": live.review_errors.load(Relaxed),
            "uptime_secs": live.started.elapsed().as_secs_f64(),
        },
    })
}

/// Framed and emitted per second over the frames computed in the last ten seconds: one
/// sample per fresh frame (the cache makes that at most five a second), the oldest kept
/// sample is the window's start, so `over_secs` says what the rate is over.
fn rate_json(live: &Live) -> Value {
    let now = std::time::Instant::now();
    let framed = live.metrics.framed.load(Relaxed);
    let emitted = live.metrics.emitted.load(Relaxed);
    let mut s = live.rate_samples.lock().unwrap_or_else(|e| e.into_inner());
    s.push_back((now, framed, emitted));
    while s.front().is_some_and(|(t, _, _)| now.duration_since(*t) > Duration::from_secs(10)) {
        s.pop_front();
    }
    let (t0, f0, e0) = s.front().copied().unwrap_or((now, framed, emitted));
    let secs = now.duration_since(t0).as_secs_f64();
    let per_sec = |then: u64, now_v: u64| if secs > 0.0 { now_v.saturating_sub(then) as f64 / secs } else { 0.0 };
    json!({ "over_secs": secs, "framed_per_sec": per_sec(f0, framed), "emitted_per_sec": per_sec(e0, emitted) })
}

fn replay_summary(live: &Live) -> Value {
    let state = live.replay.lock().unwrap_or_else(|e| e.into_inner());
    json!({
        "running": state.running.as_ref().map(|(p, done)| json!({ "version": p.version, "done": done.load(Relaxed), "total": p.total, "started": p.started })),
        "last_version": state.last.as_ref().map(|r| r.version),
        "last_error": state.last_error,
    })
}

fn replay_error(e: ReplayError) -> ApiError {
    match e {
        ReplayError::Running => ApiError::new(StatusCode::CONFLICT, "conflict", "a replay is already running"),
        ReplayError::Invalid(m) => ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, "invalid", m),
        ReplayError::Io(m) => ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "io", m),
    }
}

// ---- handlers -----------------------------------------------------------------------

async fn status(State(app): State<App>) -> Json<Value> {
    let live = &app.live;
    let pipeline = live.pipeline();
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
        // the JSON Lines file is always the output; parquet is an additional sink
        "output_format": "jsonl",
        "parquet": live.parquet,
        "watch": live.watch,
        "threads": live.threads,
        "pivot_index": live.index_entities,
        "queue_capacity": live.queue_cap,
        "tail_capacity": live.tail.capacity(),
        "infer_threshold": live.inference.threshold,
        "schema": {
            "name": pipeline.mapping.schema_name(),
            "version": pipeline.mapping.file().schema.version,
            "entities": serde_json::to_value(pipeline.mapping.entities()).unwrap_or(Value::Null),
        },
        "syslog": { "udp": live.syslog_bound.lock().unwrap_or_else(|e| e.into_inner()).0.map(|a| a.to_string()), "tcp": live.syslog_bound.lock().unwrap_or_else(|e| e.into_inner()).1.map(|a| a.to_string()), "udp_rcvbuf": live.syslog_udp_rcvbuf.load(Relaxed) },
    }))
}

async fn metrics(State(app): State<App>) -> Json<Value> {
    Json(cached_frame(&app))
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
                json!({ "id": p.id, "source": p.source, "name": p.name, "created": created, "lines": p.lines, "templates": p.templates, "unmatched": p.unmatched, "edited": p.edited, "problems": p.problems, "updates": p.updates, "version": p.version, "current_version": p.current_version })
            })
            .collect(),
    ))
}

async fn pending_get(State(app): State<App>, Path(id): Path<String>) -> Result<Json<Value>, ApiError> {
    let pending = app.live.pending_or_err().map_err(|e| review_error(&app.live, e))?;
    let d = pending.get(&id).map_err(|e| review_error(&app.live, e))?;
    // the evidence is what the engine produced; the time it was produced is review state
    let mut evidence = serde_json::to_value(&d.record.evidence).unwrap_or(Value::Null);
    let mut generated = String::new();
    ulpf_time::format_rfc3339(d.record.created_nanos, &mut generated);
    if let Some(obj) = evidence.as_object_mut() {
        obj.insert("generated".into(), Value::String(generated));
    }
    let (current_definition, diff) = pending.current_and_diff(&id, &app.live.parsers_dir);
    let version = toml::from_str::<ulpf_parse::def::ParserDefinition>(&d.definition).ok().map(|p| p.parser.version).unwrap_or(1);
    Ok(Json(json!({
        "id": d.id, "source": d.source, "definition": d.definition, "problems": d.problems, "evidence": evidence, "edited": d.record.edited,
        "updates": d.record.updates.as_ref().map(|u| u.name.clone()),
        "update_kind": d.record.updates.as_ref().map(|u| u.kind.clone()),
        "version": version,
        "current_version": d.record.updates.as_ref().map(|u| u.current_version),
        "current_definition": current_definition,
        "diff": diff,
    })))
}

async fn drift(State(app): State<App>) -> Json<Value> {
    Json(serde_json::to_value(app.live.drift_alerts()).unwrap_or(Value::Null))
}

async fn integrity(State(app): State<App>) -> Json<Value> {
    Json(app.live.integrity_summary())
}

async fn integrity_verify(State(app): State<App>) -> Result<Json<Value>, ApiError> {
    match app.live.start_verify() {
        Ok(records) => Ok(Json(json!({ "started": true, "records": records }))),
        Err(IntegrityError::Running) => Err(ApiError::new(StatusCode::CONFLICT, "conflict", "a verify is already running")),
        Err(IntegrityError::Io(m)) => Err(ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "io", m)),
    }
}

async fn attestation(State(app): State<App>) -> Result<Json<Value>, ApiError> {
    let att = app.live.attestation().map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "io", e.to_string()))?;
    Ok(Json(serde_json::to_value(att).unwrap_or(Value::Null)))
}

#[derive(Deserialize)]
struct PivotParams {
    kind: String,
    value: String,
    limit: Option<usize>,
    before: Option<i64>,
    before_id: Option<u64>,
    after: Option<i64>,
    after_id: Option<u64>,
    order: Option<String>,
}

async fn pivot(State(app): State<App>, Query(q): Query<PivotParams>) -> Result<Json<Value>, ApiError> {
    let kind = EntityKind::from_name(&q.kind).ok_or_else(|| ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, "invalid", format!("unknown entity kind `{}`; one of {}", q.kind, EntityKind::ALL.iter().map(|k| k.name()).collect::<Vec<_>>().join(", "))))?;
    let order = if q.order.as_deref() == Some("asc") { Order::Asc } else { Order::Desc };
    let page = app.live.pivot(&PivotQuery { kind, value: q.value.as_bytes(), limit: q.limit.unwrap_or(200).clamp(1, 500), before: q.before, before_id: q.before_id, after: q.after, after_id: q.after_id, order }).map_err(|e| ApiError::new(StatusCode::NOT_FOUND, "not_found", format!("pivot index: {e:#}")))?;
    Ok(Json(serde_json::to_value(page).unwrap_or(Value::Null)))
}

#[derive(Deserialize)]
struct EntitiesParams {
    kind: Option<String>,
    q: Option<String>,
    limit: Option<usize>,
}

async fn entities(State(app): State<App>, Query(p): Query<EntitiesParams>) -> Result<Json<Value>, ApiError> {
    let kind = match &p.kind {
        Some(k) => Some(EntityKind::from_name(k).ok_or_else(|| ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, "invalid", format!("unknown entity kind `{k}`")))?),
        None => None,
    };
    let list = app.live.entities(kind, p.q.as_deref().unwrap_or(""), p.limit.unwrap_or(50).clamp(1, 100)).map_err(|e| ApiError::new(StatusCode::NOT_FOUND, "not_found", format!("pivot index: {e:#}")))?;
    Ok(Json(json!({ "entities": list })))
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

#[derive(Deserialize)]
struct TraceQuery {
    bytes: Option<u8>,
}

fn trace_error(raw_id: u64, e: TracebackError) -> ApiError {
    match e {
        TracebackError::NotFound { store_len } => ApiError { status: StatusCode::NOT_FOUND, reason: "not_found", error: format!("raw id {raw_id} was never issued (store holds {store_len})"), extra: json!({ "store_len": store_len }) },
        TracebackError::Io(e) => ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "io", e),
    }
}

async fn traceback(State(app): State<App>, Path(raw_id): Path<u64>, Query(q): Query<TraceQuery>) -> Result<Json<Value>, ApiError> {
    let t = app.live.traceback_with(raw_id, q.bytes != Some(0)).map_err(|e| trace_error(raw_id, e))?;
    Ok(Json(serde_json::to_value(t).unwrap_or(Value::Null)))
}

async fn traceback_bytes(State(app): State<App>, Path(raw_id): Path<u64>) -> Result<Response, ApiError> {
    let bytes = app.live.raw_bytes(raw_id).map_err(|e| trace_error(raw_id, e))?;
    Ok(([(header::CONTENT_TYPE, "application/octet-stream")], bytes).into_response())
}

// ---- export ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct ExportQuery {
    format: Option<String>,
    from: Option<u64>,
    to: Option<u64>,
    q: Option<String>,
}

/// The eleven Parquet columns (D64), in order; the five entity columns come from the
/// schema's own entity paths, so the CSV means the same thing under ocsf and ecs.
const CSV_HEADER: &str = "raw_id,time,parser,source,class_uid,normalized,src_ip,dst_ip,user,device,dst_port\n";

fn csv_cell(out: &mut Vec<u8>, v: Option<&Value>) {
    let text = match v {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
    };
    csv_text(out, &text);
}

fn csv_text(out: &mut Vec<u8>, text: &str) {
    if text.bytes().any(|b| matches!(b, b'"' | b',' | b'\n' | b'\r')) {
        out.push(b'"');
        out.extend_from_slice(text.replace('"', "\"\"").as_bytes());
        out.push(b'"');
    } else {
        out.extend_from_slice(text.as_bytes());
    }
}

fn csv_row(out: &mut Vec<u8>, line: &[u8], paths: &[Option<String>; 5]) {
    let v: Value = match serde_json::from_slice(line) {
        Ok(v) => v,
        Err(_) => return,
    };
    let at = |path: &str| path.split('.').try_fold(&v, |o, k| o.get(k));
    let entity = |i: usize| paths[i].as_deref().and_then(at);
    let source = at("metadata.log_name");
    csv_cell(out, at("ulpf.raw_id"));
    out.push(b',');
    csv_cell(out, at("time"));
    out.push(b',');
    csv_cell(out, at("ulpf.parser"));
    out.push(b',');
    csv_cell(out, source);
    out.push(b',');
    csv_cell(out, at("class_uid"));
    out.push(b',');
    csv_text(out, std::str::from_utf8(line).unwrap_or(""));
    out.push(b',');
    csv_cell(out, entity(0));
    out.push(b',');
    csv_cell(out, entity(1));
    out.push(b',');
    csv_cell(out, entity(2));
    out.push(b',');
    csv_cell(out, entity(4).or(source));
    out.push(b',');
    csv_cell(out, entity(3));
    out.push(b'\n');
}

/// Streams the live output from the file the sink wrote (docs/api.md, Export): never the
/// tail, never a re-parse, never the whole file in memory; bounded to what was on disk
/// and terminated when the request began.
async fn export(State(app): State<App>, Query(q): Query<ExportQuery>) -> Result<Response, ApiError> {
    let live = Arc::clone(&app.live);
    if output_is_sink(&live.output) {
        return Err(ApiError::new(StatusCode::NOT_FOUND, "not_found", format!("the output {} is a device; there is no file to export", live.output.display())));
    }
    let csv = match q.format.as_deref() {
        None | Some("jsonl") => false,
        Some("csv") => true,
        Some(other) => return Err(ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, "invalid", format!("unknown format `{other}`; jsonl or csv"))),
    };
    let mut out = Output::open(&live.output).map_err(|e| ApiError::new(StatusCode::NOT_FOUND, "not_found", format!("output {}: {e}", live.output.display())))?;
    let terms: Vec<Vec<u8>> = q.q.as_deref().unwrap_or("").split_whitespace().map(|t| t.to_lowercase().into_bytes()).collect();
    let paths: [Option<String>; 5] = {
        let e = live.pipeline().mapping.entities().clone();
        [e.src_ip, e.dst_ip, e.user, e.dst_port, e.device]
    };
    let (from, to) = (q.from, q.to);
    let stem = live.output.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "out".into());
    let name = format!("{stem}-{}-{}.{}", from.map(|f| f.to_string()).unwrap_or_else(|| "first".into()), to.map(|t| t.to_string()).unwrap_or_else(|| "last".into()), if csv { "csv" } else { "jsonl" });
    let (tx, rx) = tokio::sync::mpsc::channel::<Bytes>(4);
    tokio::task::spawn_blocking(move || {
        let mut chunk: Vec<u8> = Vec::with_capacity(1 << 18);
        if csv {
            chunk.extend_from_slice(CSV_HEADER.as_bytes());
        }
        let mut at = match from {
            Some(f) => match out.lower_bound(f) {
                Ok(a) => a,
                Err(_) => return,
            },
            None => 0,
        };
        while let Ok(Some(line)) = out.line_at(at) {
            at += line.len() as u64;
            if let (Some(t), Some(i)) = (to, line_id(&line))
                && i > t
            {
                break;
            }
            if !terms.is_empty() {
                let lower = line.to_ascii_lowercase();
                if !terms.iter().all(|t| memchr::memmem::find(&lower, t).is_some()) {
                    continue;
                }
            }
            if csv {
                csv_row(&mut chunk, &line[..line.len() - 1], &paths);
            } else {
                chunk.extend_from_slice(&line);
            }
            if chunk.len() >= 1 << 18 && tx.blocking_send(Bytes::from(std::mem::take(&mut chunk))).is_err() {
                return;
            }
        }
        if !chunk.is_empty() {
            let _ = tx.blocking_send(Bytes::from(chunk));
        }
    });
    let stream = futures_util::stream::unfold(rx, |mut rx| async move { rx.recv().await.map(|c| (Ok::<Bytes, Infallible>(c), rx)) });
    let headers = [
        (header::CONTENT_TYPE, if csv { "text/csv; charset=utf-8".to_string() } else { "application/x-ndjson".to_string() }),
        (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{name}\"")),
    ];
    Ok((headers, Body::from_stream(stream)).into_response())
}

// ---- stream -------------------------------------------------------------------------

async fn replay_get(State(app): State<App>) -> Json<Value> {
    let live = &app.live;
    let versions = crate::replay::Versions::new(&live.output).list();
    let state = live.replay.lock().unwrap_or_else(|e| e.into_inner());
    Json(json!({
        "versions": versions,
        "running": state.running.as_ref().map(|(p, done)| json!({ "version": p.version, "done": done.load(Relaxed), "total": p.total, "started": p.started })),
        "last": state.last,
        "last_error": state.last_error,
    }))
}

#[derive(Deserialize, Default)]
struct ReplayBody {
    schema: Option<String>,
}

async fn replay_post(State(app): State<App>, body: Option<Json<ReplayBody>>) -> Result<Json<Value>, ApiError> {
    let body = body.map(|b| b.0).unwrap_or_default();
    let (version, total) = app.live.start_replay(body.schema.as_deref()).map_err(replay_error)?;
    Ok(Json(json!({ "version": version, "started": true, "total": total })))
}

#[derive(Deserialize)]
struct DiffQuery {
    after: Option<u64>,
    limit: Option<usize>,
    kind: Option<String>,
}

async fn replay_diff(State(app): State<App>, Path(version): Path<u64>, Query(q): Query<DiffQuery>) -> Result<Json<Value>, ApiError> {
    let (entries, next_after) = app.live.replay_diff(version, q.after, q.limit.unwrap_or(100), q.kind.as_deref()).map_err(|e| match e {
        ReplayError::Invalid(m) => ApiError::new(StatusCode::NOT_FOUND, "not_found", m),
        other => replay_error(other),
    })?;
    Ok(Json(json!({ "entries": entries, "next_after": next_after })))
}

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
    app: App,
    guard: ClientGuard,
    queue: VecDeque<Event>,
    last_id: Option<u64>,
    pending_generation: u64,
    replay_generation: u64,
    drift_generation: u64,
    integrity_generation: u64,
    tick: u64,
    initial: usize,
}

const TICK: Duration = Duration::from_millis(250);
const TAIL_PER_TICK: usize = 200;

async fn stream(State(app): State<App>, Query(q): Query<StreamQuery>) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    app.live.sse_clients.fetch_add(1, Relaxed);
    let state = StreamState { guard: ClientGuard(Arc::clone(&app.live)), queue: VecDeque::new(), last_id: None, pending_generation: app.live.pending_generation.load(Relaxed), replay_generation: app.live.replay_generation.load(Relaxed), drift_generation: app.live.drift_generation.load(Relaxed), integrity_generation: app.live.integrity_generation.load(Relaxed), tick: 0, initial: q.tail.unwrap_or(100).clamp(1, 500), app };
    let stream = futures_util::stream::unfold(state, |mut st| async move {
        loop {
            if let Some(ev) = st.queue.pop_front() {
                return Some((Ok::<Event, Infallible>(ev), st));
            }
            let live = &st.guard.0;
            if st.tick == 0 {
                let frame = live.tail.since(None, st.initial);
                st.last_id = frame.latest;
                let count = live.pending.as_ref().map(|p| p.ids().len()).unwrap_or(0);
                let hello = json!({ "latest_raw_id": frame.latest, "pending_generation": st.pending_generation, "pending_count": count, "tail": tail_json(frame) });
                st.queue.push_back(event("hello", &hello));
                st.queue.push_back(event("metrics", &cached_frame(&st.app)));
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
                st.queue.push_back(event("metrics", &cached_frame(&st.app)));
            }
            let generation = live.pending_generation.load(Relaxed);
            if generation != st.pending_generation {
                st.pending_generation = generation;
                let count = live.pending.as_ref().map(|p| p.ids().len()).unwrap_or(0);
                st.queue.push_back(event("pending", &json!({ "generation": generation, "count": count })));
            }
            let igen = live.integrity_generation.load(Relaxed);
            if igen != st.integrity_generation {
                st.integrity_generation = igen;
                st.queue.push_back(event("integrity", &live.integrity_summary()));
            }
            let dgen = live.drift_generation.load(Relaxed);
            if dgen != st.drift_generation {
                st.drift_generation = dgen;
                for alert in live.drift_alerts().into_iter().filter(|a| a.state != DriftState::Watching) {
                    st.queue.push_back(event("drift", &serde_json::to_value(&alert).unwrap_or(Value::Null)));
                }
            }
            let rgen = live.replay_generation.load(Relaxed);
            let running = live.replay_progress();
            if rgen != st.replay_generation || (running.is_some() && st.tick % 2 == 0) {
                st.replay_generation = rgen;
                let state = live.replay.lock().unwrap_or_else(|e| e.into_inner());
                let frame = match (&running, &state.last, &state.last_error) {
                    (Some(p), _, _) => json!({ "version": p.version, "state": if p.done == 0 { "started" } else { "progress" }, "done": p.done, "total": p.total, "report": Value::Null, "error": Value::Null }),
                    (None, _, Some(e)) => json!({ "version": Value::Null, "state": "failed", "done": 0, "total": 0, "report": Value::Null, "error": e }),
                    (None, Some(r), None) => json!({ "version": r.version, "state": "done", "done": r.events, "total": r.events, "report": r, "error": Value::Null }),
                    (None, None, None) => Value::Null,
                };
                if !frame.is_null() {
                    st.queue.push_back(event("replay", &frame));
                }
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
