//! HTTP server: 3-endpoint JSON API + bearer-token auth. Web UI (static
//! assets) is deferred to C4.

use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{ConnectInfo, Path as AxumPath, Request, State};
use axum::http::{header, HeaderValue, StatusCode, Uri};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{get, post};
use axum::Router;
use rust_embed::RustEmbed;
use serde::Serialize;
use tokio::sync::Mutex;

#[derive(RustEmbed)]
#[folder = "web/"]
struct WebAssets;

use crate::about;
use crate::audio::{self, AudioDevice};
use crate::config::{Config, SortPolicy};
use crate::history::History;
use crate::net;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub history: Arc<Mutex<History>>,
    pub history_path: Arc<PathBuf>,
}

pub async fn serve(state: AppState) -> Result<(), std::io::Error> {
    let addr: SocketAddr = format!("{}:{}", state.config.server.bind, state.config.server.port)
        .parse()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, format!("bind: {e}")))?;

    let app = Router::new()
        .route("/api/status", get(status_handler))
        .route("/api/devices", get(devices_handler))
        .route("/api/devices/:id/default", post(set_default_handler))
        .route("/api/about", get(about_handler))
        .route("/api/languages", get(languages_handler))
        .layer(middleware::from_fn_with_state(state.clone(), auth_middleware))
        .fallback(static_handler)
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    println!("[listening] http://{addr}/");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    println!("\n[shutdown] Ctrl+C received");
}

// ---- Handlers ---------------------------------------------------------------

#[derive(Serialize)]
struct StatusBody {
    version: &'static str,
    bind: String,
    port: u16,
    lan_exposed: bool,
    require_token: bool,
    device_sort: SortPolicy,
    /// LAN URLs with the token pre-embedded in `#t=` — the Web UI shows these
    /// as one-tap-copy in a "connect from another machine" panel. Sorted:
    /// physical NICs first, virtual switches last.
    share_urls: Vec<net::ShareEntry>,
    default_console: Option<String>,
    default_multimedia: Option<String>,
    default_communications: Option<String>,
}

async fn status_handler(State(state): State<AppState>) -> Response {
    let devices = match audio::list_devices() {
        Ok(d) => d,
        Err(e) => return audio_error(e),
    };
    let pick = |flag: fn(&AudioDevice) -> bool| -> Option<String> {
        devices.iter().find(|d| flag(d)).map(|d| d.id.clone())
    };
    let body = StatusBody {
        version: env!("CARGO_PKG_VERSION"),
        bind: state.config.server.bind.clone(),
        port: state.config.server.port,
        lan_exposed: state.config.lan_exposed(),
        require_token: state.config.auth.require_token,
        device_sort: state.config.audio.device_sort,
        share_urls: net::build_share_entries(&state.config),
        default_console: pick(|d| d.is_default_console),
        default_multimedia: pick(|d| d.is_default_multimedia),
        default_communications: pick(|d| d.is_default_communications),
    };
    Json(body).into_response()
}

#[derive(Serialize)]
struct DevicesBody {
    devices: Vec<AudioDevice>,
    sort: SortPolicy,
}

async fn devices_handler(State(state): State<AppState>) -> Response {
    let mut devices = match audio::list_devices() {
        Ok(d) => d,
        Err(e) => return audio_error(e),
    };
    let sort = state.config.audio.device_sort;
    match sort {
        SortPolicy::State => sort_by_state(&mut devices),
        SortPolicy::Name => devices.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase())),
        SortPolicy::Recent => {
            let history = state.history.lock().await;
            devices.sort_by(|a, b| {
                let ta = history.last_used_at(&a.id);
                let tb = history.last_used_at(&b.id);
                match (ta, tb) {
                    (Some(x), Some(y)) => y.cmp(&x),
                    (Some(_), None) => std::cmp::Ordering::Less,
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    // Fall back to state order when neither has history.
                    (None, None) => state_rank(a).cmp(&state_rank(b)),
                }
            });
        }
    }
    Json(DevicesBody { devices, sort }).into_response()
}

async fn set_default_handler(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    if let Err(e) = audio::set_default(&id) {
        return audio_error(e);
    }
    {
        let mut history = state.history.lock().await;
        history.touch(&id);
        if let Err(e) = crate::history::save(&state.history_path, &history) {
            eprintln!("[warn] failed to persist history.toml: {e}");
        }
    }
    (StatusCode::NO_CONTENT).into_response()
}

async fn about_handler() -> Response {
    Json(about::build()).into_response()
}

/// Enumerate language packs available under `web/lang/*.json` and return their
/// display metadata. Adding a language = dropping in a new JSON file with a
/// `_lang` block — no server-side code change required.
async fn languages_handler() -> Response {
    #[derive(serde::Serialize)]
    struct LangInfo {
        code: String,
        name: String,
    }

    let mut out: Vec<LangInfo> = Vec::new();
    for path in WebAssets::iter() {
        let p = path.as_ref();
        if !p.starts_with("lang/") || !p.ends_with(".json") {
            continue;
        }
        let code = p
            .trim_start_matches("lang/")
            .trim_end_matches(".json")
            .to_string();

        // Try to pull the friendly name from the pack itself; fall back to
        // the file's code if the pack lacks a `_lang` block.
        let mut name = code.clone();
        if let Some(file) = WebAssets::get(p) {
            if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&file.data) {
                if let Some(n) = v
                    .get("_lang")
                    .and_then(|l| l.get("name"))
                    .and_then(|s| s.as_str())
                {
                    name = n.to_string();
                }
            }
        }
        out.push(LangInfo { code, name });
    }
    out.sort_by(|a, b| a.code.cmp(&b.code));
    Json(out).into_response()
}

async fn static_handler(uri: Uri) -> Response {
    let raw = uri.path().trim_start_matches('/');
    let path = if raw.is_empty() { "index.html" } else { raw };

    if let Some(file) = WebAssets::get(path) {
        return (
            StatusCode::OK,
            [(header::CONTENT_TYPE, mime_from_path(path))],
            file.data.into_owned(),
        )
            .into_response();
    }

    // Unknown path — fall back to index.html so client-side routing works and
    // typos still land on the UI.
    if let Some(file) = WebAssets::get("index.html") {
        return (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
            file.data.into_owned(),
        )
            .into_response();
    }
    (StatusCode::NOT_FOUND, "not found").into_response()
}

fn mime_from_path(path: &str) -> &'static str {
    let ext = path.rsplit('.').next().unwrap_or("");
    match ext {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "application/javascript; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    }
}

// ---- Auth middleware --------------------------------------------------------

async fn auth_middleware(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    if !state.config.auth.require_token {
        return Ok(next.run(request).await);
    }
    // Loopback bypass: connections from 127.0.0.1 / ::1 are OS-guaranteed to
    // originate on the host itself, so requiring a token there just annoys the
    // user without adding security. LAN clients still need the token.
    if is_loopback(peer.ip()) {
        return Ok(next.run(request).await);
    }
    let header = request
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let token = header.strip_prefix("Bearer ").unwrap_or("").trim();
    if !token.is_empty() && token == state.config.auth.token {
        Ok(next.run(request).await)
    } else {
        let mut resp = (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
        resp.headers_mut().insert(
            "www-authenticate",
            HeaderValue::from_static("Bearer realm=\"audioremote\""),
        );
        Ok(resp)
    }
}

fn is_loopback(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(a) => a.is_loopback(),
        IpAddr::V6(a) => a.is_loopback(),
    }
}

// ---- Helpers ----------------------------------------------------------------

fn audio_error(e: audio::AudioError) -> Response {
    let body = serde_json::json!({
        "error": "audio",
        "context": e.context,
        "message": e.source.to_string(),
        "hresult": format!("0x{:08x}", e.source.code().0 as u32),
    });
    (StatusCode::INTERNAL_SERVER_ERROR, Json(body)).into_response()
}

fn sort_by_state(devices: &mut [AudioDevice]) {
    devices.sort_by(|a, b| state_rank(a).cmp(&state_rank(b)));
}

fn state_rank(d: &AudioDevice) -> u8 {
    use crate::audio::DeviceState::*;
    match d.state {
        Active => 0,
        Unplugged => 1,
        Disabled => 2,
        NotPresent => 3,
        Unknown => 4,
    }
}
