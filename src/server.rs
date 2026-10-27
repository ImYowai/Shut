use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use axum::body::{Body, Bytes};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, DefaultBodyLimit, Query, Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use std::net::SocketAddr;

use crate::power;
use crate::remote::{self, InputEvent};
use crate::state::{AppState, Phase, Settings, TunnelState};
use crate::video;

pub struct ServerCtx {
    pub state: AppState,
    pub port: u16,
    pub lan_url: String,
}

type Ctx = Arc<ServerCtx>;

/// Android/WebViewクライアント向けのセルフホスト操作画面。
const INDEX_HTML: &str = include_str!("../ui/index.html");
const SHELL_CSS: &str = include_str!("../ui/shell.css");
const SHELL_JS: &str = include_str!("../ui/shell.js");

pub fn router(ctx: Ctx) -> Router {
    Router::new()
        .route("/", get(index))
        .route(
            "/shell.css",
            get(|| async { ([("content-type", "text/css")], SHELL_CSS) }),
        )
        .route(
            "/shell.js",
            get(|| async { ([("content-type", "application/javascript")], SHELL_JS) }),
        )
        .route("/api/auth", post(api_auth))
        .route("/api/state", get(api_state))
        .route("/api/settings", post(api_settings))
        .route("/api/arm", post(api_arm))
        .route("/api/disarm", post(api_disarm))
        .route("/api/shutdown", post(api_shutdown))
        .route("/api/cancel", post(api_cancel))
        .route("/api/tunnel", post(api_tunnel))
        .route("/input", post(api_input))
        .route(
            "/api/clipboard",
            get(api_clipboard_get).post(api_clipboard_set),
        )
        .route(
            "/api/upload",
            post(api_upload).route_layer(DefaultBodyLimit::max(256 * 1024 * 1024)),
        )
        .route("/stream.mjpeg", get(mjpeg))
        .route("/ws/video", get(ws_video))
        .route("/qr", get(qr))
        .layer(middleware::from_fn_with_state(ctx.clone(), auth))
        .with_state(ctx)
}

// ---------- 認証 ----------

async fn auth(
    State(ctx): State<Ctx>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
    req: Request,
    next: Next,
) -> Response {
    // 静的ページと認証入口は無認証。ページはAPIを呼べないだけの殻で、
    // 機微な操作・データは全てトークン必須。
    if matches!(
        req.uri().path(),
        "/" | "/shell.css" | "/shell.js" | "/api/auth"
    ) {
        return next.run(req).await;
    }
    let candidate = q
        .get("t")
        .cloned()
        .or_else(|| {
            headers
                .get("x-shut-token")
                .and_then(|v| v.to_str().ok())
                .map(str::to_string)
        })
        .or_else(|| {
            headers
                .get(header::COOKIE)
                .and_then(|v| v.to_str().ok())
                .and_then(|c| {
                    c.split(';')
                        .find_map(|p| p.trim().strip_prefix("shut_token="))
                        .map(str::to_string)
                })
        });
    let ok = candidate.is_some_and(|t| ctx.state.lock().unwrap().auth.valid_token(&t));
    if ok {
        next.run(req).await
    } else {
        (StatusCode::UNAUTHORIZED, "unauthorized").into_response()
    }
}

// ---------- 認証 ----------

/// POST /api/auth {"password": "..."} -> {"token": "..."}
/// パスワードを argon2 ハッシュと検証し、セッショントークンを発行する。
/// 連続失敗で指数的バックオフ。トークンはプロセス生存中のみ有効。
async fn api_auth(
    State(ctx): State<Ctx>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let pw = body
        .get("password")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    if pw.is_empty() || pw.len() > 256 {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "password required"})),
        )
            .into_response();
    }
    let mut st = ctx.state.lock().unwrap();
    let hash = st.settings.password_hash.clone();
    match st.auth.attempt(peer.ip(), pw, hash.as_deref()) {
        Ok(token) => Json(json!({"ok": true, "token": token})).into_response(),
        Err(e) => {
            crate::log::line(&format!("auth failed from {peer}: {e}"));
            (StatusCode::UNAUTHORIZED, Json(json!({"error": e}))).into_response()
        }
    }
}

// ---------- ページ ----------

async fn index() -> impl IntoResponse {
    Html(INDEX_HTML)
}

// ---------- API ----------

async fn api_state(State(ctx): State<Ctx>) -> Json<serde_json::Value> {
    let st = ctx.state.lock().unwrap();
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    let (phase, reason, countdown_left, timer_deadline, timer_remain) = match &st.phase {
        Phase::Config => ("config", "".into(), 0, None, None),
        Phase::Armed { timer_deadline } => (
            "armed",
            "".into(),
            0,
            *timer_deadline,
            timer_deadline.map(|d| (d - now).max(0)),
        ),
        Phase::Countdown { deadline, reason } => (
            "countdown",
            reason.clone(),
            deadline.saturating_duration_since(Instant::now()).as_secs(),
            None,
            None,
        ),
        Phase::Done { message } => ("done", message.clone(), 0, None, None),
    };

    let fw_ago = st.fw_status.ago_secs;
    let (tunnel_status, tunnel_url, tunnel_error) = match &st.tunnel {
        TunnelState::Off => ("off", None, None),
        TunnelState::Starting => ("starting", None, None),
        TunnelState::Running(u) => ("running", Some(u.clone()), None),
        TunnelState::Failed(e) => ("failed", None, Some(e.clone())),
    };

    // 設定のうちパスワードハッシュはクライアントへ出さない（オフライン解析を防ぐ）
    let mut settings_json = serde_json::to_value(&st.settings).unwrap_or_default();
    if let Some(obj) = settings_json.as_object_mut() {
        obj.remove("password_hash");
    }

    Json(json!({
        "phase": phase,
        "reason": reason,
        "countdown_left": countdown_left,
        "timer_deadline": timer_deadline,
        "timer_remain": timer_remain,
        "fw": {
            "saw_activity": st.fw_status.saw_activity,
            "ago_secs": fw_ago,
            "idle_minutes": st.settings.file_idle_minutes,
        },
        "tunnel": {"status": tunnel_status, "url": tunnel_url, "error": tunnel_error},
        "monitors": remote::monitor_list(),
        "cursor": remote::cursor_info(),
        "mirror_clients": st.mirror_clients,
        "lan_url": ctx.lan_url,
        "port": ctx.port,
        "password_set": st.settings.password_hash.is_some(),
        "settings": settings_json,
    }))
}

async fn api_settings(
    State(ctx): State<Ctx>,
    Json(mut settings): Json<Settings>,
) -> impl IntoResponse {
    let mut st = ctx.state.lock().unwrap();
    // パスワード変更はホスト側UI/CLIのみ — APIからのハッシュ上書きは拒否
    // （認証済みセッションによる恒久的な乗っ取りを防ぐ）
    settings.password_hash = st.settings.password_hash.clone();
    st.settings = settings;
    st.settings.save();
    Json(json!({"ok": true}))
}

async fn api_arm(State(ctx): State<Ctx>) -> impl IntoResponse {
    match ctx.state.lock().unwrap().arm() {
        Ok(()) => Json(json!({"ok": true})).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, Json(json!({"error": e}))).into_response(),
    }
}

async fn api_disarm(State(ctx): State<Ctx>) -> impl IntoResponse {
    let mut st = ctx.state.lock().unwrap();
    // カウントダウン中の中止はWindows側の予約も解除する
    if matches!(st.phase, Phase::Countdown { .. }) {
        let _ = crate::power::abort();
        crate::log::line("disarm: countdown aborted");
    }
    st.phase = Phase::Config;
    Json(json!({"ok": true}))
}

async fn api_shutdown(State(ctx): State<Ctx>, body: Option<Json<serde_json::Value>>) -> Response {
    let now = body
        .and_then(|v| v.get("now").and_then(|b| b.as_bool()))
        .unwrap_or(false);
    if now {
        let force = ctx.state.lock().unwrap().settings.force_close;
        return match power::shutdown(force) {
            Ok(()) => {
                crate::log::line("shutdown now: executed");
                ctx.state.lock().unwrap().phase = Phase::Done {
                    message: "シャットダウンを実行しました".into(),
                };
                Json(json!({"ok": true})).into_response()
            }
            Err(e) => {
                crate::log::line(&format!("shutdown now: failed {e}"));
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error": e.to_string()})),
                )
                    .into_response()
            }
        };
    }
    ctx.state
        .lock()
        .unwrap()
        .fire("リモートからのシャットダウン".into());
    Json(json!({"ok": true})).into_response()
}

async fn api_cancel(State(ctx): State<Ctx>) -> impl IntoResponse {
    let _ = power::abort();
    crate::log::line("cancel");
    ctx.state.lock().unwrap().phase = Phase::Config;
    Json(json!({"ok": true}))
}

async fn api_tunnel(State(ctx): State<Ctx>) -> impl IntoResponse {
    {
        let st = ctx.state.lock().unwrap();
        if matches!(st.tunnel, TunnelState::Starting | TunnelState::Running(_)) {
            drop(st);
            remote::stop_tunnel(&ctx.state);
            return Json(json!({"ok": true}));
        }
    }
    remote::start_tunnel(&ctx.state, ctx.port);
    Json(json!({"ok": true}))
}

async fn api_input(State(ctx): State<Ctx>, Json(ev): Json<InputEvent>) -> impl IntoResponse {
    if !ctx.state.lock().unwrap().settings.control_enabled {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"error": "control disabled"})),
        )
            .into_response();
    }
    // SendInputは呼び出しスレッドをブロックするため、ワーカースレッドで実行する
    let result = tokio::task::spawn_blocking(move || remote::handle_input(&ev)).await;
    match result {
        Ok(Ok(())) => Json(json!({"ok": true})).into_response(),
        Ok(Err(error)) => {
            let status = match error {
                remote::InputError::Invalid(_) => StatusCode::BAD_REQUEST,
                remote::InputError::Injection => StatusCode::UNPROCESSABLE_ENTITY,
            };
            (status, Json(json!({"error": error.message()}))).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("入力スレッドの実行に失敗: {e}")})),
        )
            .into_response(),
    }
}

// ---------- H.264 ライブストリーム (WebSocket) ----------
//
// 下り: 接続直後にJSON設定 -> 以後 [flags][Annex-B] のバイナリ連打
// 上り: テキストJSONの InputEvent (HTTP POST /input と等価だが遅延が小さい)

async fn ws_video(ws: WebSocketUpgrade, State(ctx): State<Ctx>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| video_socket(socket, ctx))
}

async fn video_socket(mut socket: WebSocket, ctx: Ctx) {
    if !ctx.state.lock().unwrap().settings.mirror_enabled {
        let _ = socket
            .send(Message::Text(
                r#"{"type":"error","error":"mirror disabled"}"#.into(),
            ))
            .await;
        return;
    }
    let hub = video::hub();
    let Some((w, h)) = hub.dims().await else {
        let _ = socket
            .send(Message::Text(
                r#"{"type":"error","error":"video encoder unavailable"}"#.into(),
            ))
            .await;
        return;
    };

    let control = ctx.state.lock().unwrap().settings.control_enabled;
    let mut rx = hub.subscribe();
    hub.kick(); // 新規購読者のために次フレームをIDRにする
    ctx.state.lock().unwrap().mirror_clients += 1;
    let _guard = ClientGuard(ctx.state.clone());

    let (mut sink, mut stream) = socket.split();
    let writer = tokio::spawn(async move {
        if sink
            .send(Message::Text(
                json!({"type":"config","codec":"h264-annexb","w":w,"h":h})
                    .to_string()
                    .into(),
            ))
            .await
            .is_err()
        {
            return;
        }
        loop {
            match rx.recv().await {
                Ok(pkt) => {
                    if sink.send(Message::Binary(pkt.into())).await.is_err() {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    continue; // 古いフレームは捨てて最新へ
                }
                Err(_) => break,
            }
        }
    });

    // 上り: テキストJSONの入力イベント (control_enabledの時だけ適用)
    while let Some(msg) = stream.next().await {
        match msg {
            Ok(Message::Text(t)) => {
                if !control {
                    continue;
                }
                if let Ok(ev) = serde_json::from_str::<InputEvent>(&t) {
                    let _ = tokio::task::spawn_blocking(move || remote::handle_input(&ev)).await;
                }
            }
            Ok(Message::Close(_)) | Err(_) => break,
            _ => {}
        }
    }
    writer.abort();
}

// ---------- クリップボード・ファイル ----------

async fn api_clipboard_get() -> impl IntoResponse {
    match tokio::task::spawn_blocking(remote::clipboard_get).await {
        Ok(Ok(text)) => Json(json!({"ok": true, "text": text})).into_response(),
        Ok(Err(e)) => (StatusCode::UNPROCESSABLE_ENTITY, Json(json!({"error": e}))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn api_clipboard_set(Json(body): Json<serde_json::Value>) -> impl IntoResponse {
    let Some(text) = body.get("text").and_then(|v| v.as_str()) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "text が必要です"})),
        )
            .into_response();
    };
    if text.encode_utf16().count() > 262144 {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "長すぎます（256Ki UTF-16単位まで）"})),
        )
            .into_response();
    }
    let text = text.to_string();
    match tokio::task::spawn_blocking(move || remote::clipboard_set(&text)).await {
        Ok(Ok(())) => Json(json!({"ok": true})).into_response(),
        Ok(Err(e)) => (StatusCode::UNPROCESSABLE_ENTITY, Json(json!({"error": e}))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

/// 端末からPCへファイルを保存する。保存先は %LOCALAPPDATA%\Shut\incoming
async fn api_upload(Query(q): Query<HashMap<String, String>>, body: Bytes) -> impl IntoResponse {
    let raw = q.get("name").map(|s| s.as_str()).unwrap_or("file.bin");
    // パス区切り・危険文字を除いたファイル名だけを使う
    let base = raw
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or("file.bin")
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect::<String>()
        .trim_matches([' ', '.'])
        .chars()
        .take(120)
        .collect::<String>();
    let name = if base.is_empty() { "file.bin" } else { &base };
    let dir = crate::data_dir().join("incoming");
    if std::fs::create_dir_all(&dir).is_err() {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "保存先を作成できませんでした"})),
        )
            .into_response();
    }
    // 同名は -1, -2 … を付ける
    let mut path = dir.join(name);
    for i in 1..1000 {
        if !path.exists() {
            break;
        }
        let stem = std::path::Path::new(name)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("file");
        let ext = std::path::Path::new(name)
            .extension()
            .and_then(|s| s.to_str())
            .map(|e| format!(".{e}"))
            .unwrap_or_default();
        path = dir.join(format!("{stem}-{i}{ext}"));
    }
    let path_str = path.to_string_lossy().to_string();
    let len = body.len();
    match tokio::task::spawn_blocking(move || std::fs::write(&path, &body)).await {
        Ok(Ok(())) => Json(json!({
            "ok": true,
            "path": path_str,
            "bytes": len,
        }))
        .into_response(),
        Ok(Err(e)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("保存に失敗しました: {e}")})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("保存に失敗しました: {e}")})),
        )
            .into_response(),
    }
}

// ---------- ミラー配信 ----------

/// クライアント数カウント用ガード
struct ClientGuard(AppState);
impl Drop for ClientGuard {
    fn drop(&mut self) {
        let mut st = self.0.lock().unwrap();
        st.mirror_clients = st.mirror_clients.saturating_sub(1);
    }
}

async fn mjpeg(
    State(ctx): State<Ctx>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    if !ctx.state.lock().unwrap().settings.mirror_enabled {
        return (StatusCode::FORBIDDEN, "mirror disabled").into_response();
    }
    ctx.state.lock().unwrap().mirror_clients += 1;
    let state = ctx.state.clone();
    let m: usize = params.get("m").and_then(|v| v.parse().ok()).unwrap_or(0);
    let w: u32 = params
        .get("w")
        .and_then(|v| v.parse().ok())
        .unwrap_or(960)
        .clamp(320, 2560);
    let q: u8 = params
        .get("q")
        .and_then(|v| v.parse().ok())
        .unwrap_or(55)
        .clamp(20, 95);

    let stream = async_stream::stream! {
        let _guard = ClientGuard(state);
        const FRAME_BUDGET: Duration = Duration::from_millis(30); // 目標~33fps
        let mut last_hash = 0u64;
        loop {
            let started = std::time::Instant::now();
            let frame = tokio::task::spawn_blocking(move || remote::capture_jpeg(m, w, q))
                .await
                .ok()
                .flatten();
            if let Some((jpeg, hash)) = frame {
                if hash != last_hash {
                    last_hash = hash;
                    let head = format!(
                        "--frame\r\nContent-Type: image/jpeg\r\nContent-Length: {}\r\n\r\n",
                        jpeg.len()
                    );
                    yield Ok::<_, std::io::Error>(head.into_bytes());
                    yield Ok(jpeg);
                    yield Ok(b"\r\n".to_vec());
                }
                // キャプチャ+エンコードに掛かった時間を引いた分だけ待つ
                let elapsed = started.elapsed();
                if elapsed < FRAME_BUDGET {
                    tokio::time::sleep(FRAME_BUDGET - elapsed).await;
                }
            } else {
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        }
    };

    Response::builder()
        .header(
            header::CONTENT_TYPE,
            "multipart/x-mixed-replace;boundary=frame",
        )
        .header(header::CACHE_CONTROL, "no-cache")
        .body(Body::from_stream(stream))
        .unwrap()
        .into_response()
}

// ---------- QR ----------

#[allow(dead_code)]
async fn qr(Query(q): Query<HashMap<String, String>>) -> impl IntoResponse {
    let Some(text) = q.get("u") else {
        return (StatusCode::BAD_REQUEST, "u required").into_response();
    };
    let Ok(code) = qrcode::QrCode::new(text.as_bytes()) else {
        return (StatusCode::BAD_REQUEST, "too long").into_response();
    };
    // imageクレート版違いを避けるため自前でLumaバッファを生成
    let width = code.width();
    let colors = code.to_colors();
    const SCALE: usize = 8;
    const QUIET: usize = 4;
    let dim = (width + QUIET * 2) * SCALE;
    let mut luma = vec![255u8; dim * dim];
    for y in 0..width {
        for x in 0..width {
            if colors[y * width + x] == qrcode::Color::Dark {
                for yy in 0..SCALE {
                    let row = (y + QUIET) * SCALE + yy;
                    let start = row * dim + (x + QUIET) * SCALE;
                    luma[start..start + SCALE].fill(0);
                }
            }
        }
    }
    use image::ImageEncoder;
    let mut buf = Vec::new();
    if image::codecs::png::PngEncoder::new(&mut buf)
        .write_image(&luma, dim as u32, dim as u32, image::ColorType::L8)
        .is_err()
    {
        return (StatusCode::INTERNAL_SERVER_ERROR, "encode failed").into_response();
    }
    ([(header::CONTENT_TYPE, "image/png")], buf).into_response()
}
