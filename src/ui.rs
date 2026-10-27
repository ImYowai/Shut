//! Dioxus UI。起動時のモード選択 → ホスト画面 / クライアント画面。

use std::sync::mpsc::Receiver;
use std::time::Duration;

use dioxus::html::geometry::WheelDelta;
use dioxus::html::input_data::MouseButton;
use dioxus::prelude::*;
use shut_protocol::{InputEvent, validate_connection};

use crate::clientnet::{self, Job, Remote};
use crate::state::{AppState, Phase, TunnelState};
use crate::{auth, framesrv, host, remote as ops, startup};

/// 起動設定。main から with_context で注入される。
#[derive(Clone)]
pub struct Boot {
    pub force_host: bool,
    pub state: AppState,
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Menu,
    Host,
    Client,
}

/// `shut <url>` / SHUT_REMOTE_URL 指定の自動接続URLを返す。
fn auto_url() -> Option<String> {
    std::env::var("SHUT_REMOTE_URL")
        .ok()
        .or_else(|| std::env::args().nth(1))
        .filter(|a| a.contains("://"))
}

#[component]
pub fn App() -> Element {
    let boot = use_context::<Boot>();
    let mut mode = use_signal(|| {
        if boot.force_host {
            Mode::Host
        } else if auto_url().is_some() {
            Mode::Client
        } else {
            Mode::Menu
        }
    });

    rsx! {
        style { dangerous_inner_html: CSS }
        document::Title { "Shut" }
        match mode() {
            Mode::Menu => rsx! { Menu { on_pick: move |m| mode.set(m) } },
            Mode::Host => rsx! {
                Host { on_back: move |_| mode.set(Mode::Menu) }
            },
            Mode::Client => rsx! { Client { on_back: move |_| mode.set(Mode::Menu) } },
        }
    }
}

// ---------- モード選択 ----------

#[component]
fn Menu(on_pick: EventHandler<Mode>) -> Element {
    rsx! {
        main { class: "shell menu-shell",
            header { class: "topbar",
                div { class: "brand", span { class: "brand-dot" } "Shut" }
                span { class: "pill", "リモート & スケジュールシャットダウン" }
            }
            section { class: "intro",
                h1 { "どちらで使いますか？" }
                p { "このPCを他から操作できるようにするか、別のPCへ接続するかを選べます。" }
            }
            div { class: "menu-cards",
                button {
                    class: "menu-card",
                    onclick: move |_| on_pick.call(Mode::Host),
                    div { class: "menu-icon", "◈" }
                    h2 { "このPCをホストにする" }
                    p { "別のデバイスからこの画面を見て操作できるようになります。スケジュールシャットダウンもこちら側で動きます。" }
                    span { class: "menu-go", "ホストとして起動 →" }
                }
                button {
                    class: "menu-card alt",
                    onclick: move |_| on_pick.call(Mode::Client),
                    div { class: "menu-icon", "⌁" }
                    h2 { "他のPCへ接続する" }
                    p { "ホスト側のPCに表示されたアドレスへ接続して、画面を見てそのまま操作します。" }
                    span { class: "menu-go", "接続クライアントとして起動 →" }
                }
            }
            footer { class: "foot",
                span { class: "privacy-dot" }
                "通信は同一ネットワーク内の直接接続です。パスワードはハッシュ化されて保存されます。"
            }
        }
    }
}

// ---------- ホスト画面 ----------

/// パスワードを設定して保存する。戻り値は表示メッセージ。
fn apply_password(state: &AppState, pw: &str) -> Result<&'static str, String> {
    if pw.chars().count() < 6 {
        return Err("6文字以上にしてください".into());
    }
    let h = auth::hash_password(pw)?;
    let mut st = state.lock().unwrap();
    st.settings.password_hash = Some(h);
    st.settings.save();
    Ok("設定しました")
}

#[component]
fn Host(on_back: EventHandler) -> Element {
    let state = use_context::<Boot>().state;
    let mut info = use_signal(|| None::<Result<host::HostInfo, String>>);
    use_hook({
        let state = state.clone();
        move || {
            info.set(Some(host::start(&state)));
        }
    });

    // 1秒ごとに状態を再描画
    let mut tick = use_signal(|| 0u64);
    use_future(move || async move {
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            tick += 1;
        }
    });
    let _ = tick();

    let mut pw_input = use_signal(String::new);
    let mut pw_msg = use_signal(String::new);
    let mut startup_on = use_signal(|| startup::registered().is_some());
    let mut settings_msg = use_signal(String::new);

    let snap = {
        let st = state.lock().unwrap();
        Snap {
            phase: st.phase.clone(),
            password_set: st.settings.password_hash.is_some(),
            mirror: st.settings.mirror_enabled,
            control: st.settings.control_enabled,
            clients: st.mirror_clients,
            tunnel: st.tunnel.clone(),
            force_close: st.settings.force_close,
            countdown_secs: st.settings.countdown_secs,
            fw_ago: st.fw_status.ago_secs,
        }
    };

    let qr = info.read().as_ref().and_then(|r| {
        r.as_ref().ok().and_then(|i| qr_data_url(&i.lan_url))
    });
    let lan_url = info.read().as_ref().and_then(|r| {
        r.as_ref().ok().map(|i| i.lan_url.clone())
    });
    let start_err = info.read().as_ref().and_then(|r| {
        r.as_ref().err().cloned()
    });

    macro_rules! pw_submit {
        () => {{
            let state = state.clone();
            move |_| match apply_password(&state, &pw_input()) {
                Ok(m) => {
                    pw_input.set(String::new());
                    pw_msg.set(m.into());
                }
                Err(e) => pw_msg.set(e),
            }
        }};
    }

    rsx! {
        main { class: "shell",
            header { class: "topbar",
                div { class: "brand", span { class: "brand-dot" } "Shut" }
                span { class: "pill", "ホスト" }
                button { class: "quiet back", onclick: move |_| on_back.call(()), "← メニュー" }
            }

            if let Some(e) = start_err {
                section { class: "card",
                    p { class: "error", "サーバーを起動できませんでした: {e}" }
                }
            }

            if !snap.password_set {
                section { class: "card warn-card",
                    h3 { "接続パスワードを設定してください" }
                    p { "パスワードが無いホストは接続を一切受け付けません。最初に設定します。" }
                    div { class: "input-row",
                        input {
                            r#type: "password",
                            placeholder: "6文字以上",
                            value: "{pw_input}",
                            oninput: move |e| pw_input.set(e.value()),
                            onkeydown: {
                                let mut f = pw_submit!();
                                move |e| {
                                    if e.key() == Key::Enter {
                                        f(())
                                    }
                                }
                            },
                        }
                        button { class: "primary", onclick: pw_submit!(), "設定" }
                    }
                    if !pw_msg().is_empty() {
                        p { class: "notice", "{pw_msg}" }
                    }
                }
            }

            section { class: "card host-url-card",
                h3 { "接続アドレス" }
                if let Some(url) = lan_url {
                    div { class: "url-row",
                        code { class: "big-url", "{url}" }
                        if let Some(q) = qr {
                            img { class: "qr", src: "{q}", alt: "接続QR" }
                        }
                    }
                    p { class: "hint",
                        "クライアント側でこのアドレスと、設定したパスワードを入力します。"
                    }
                } else {
                    p { "サーバーを起動しています…" }
                }
            }

            section { class: "card",
                h3 { "接続の許可" }
                label { class: "check-row",
                    input {
                        r#type: "checkbox",
                        checked: snap.mirror,
                        onchange: {
                            let state = state.clone();
                            move |e| {
                                let mut st = state.lock().unwrap();
                                st.settings.mirror_enabled = e.checked();
                                st.settings.save();
                            }
                        },
                    }
                    "画面の配信を許可する"
                }
                label { class: "check-row",
                    input {
                        r#type: "checkbox",
                        checked: snap.control,
                        onchange: {
                            let state = state.clone();
                            move |e| {
                                let mut st = state.lock().unwrap();
                                st.settings.control_enabled = e.checked();
                                st.settings.save();
                            }
                        },
                    }
                    "マウス・キーボード操作を許可する"
                }
                label { class: "check-row",
                    input {
                        r#type: "checkbox",
                        checked: snap.force_close,
                        onchange: {
                            let state = state.clone();
                            move |e| {
                                let mut st = state.lock().unwrap();
                                st.settings.force_close = e.checked();
                                st.settings.save();
                            }
                        },
                    }
                    "シャットダウン時にアプリを強制終了する"
                }
                p { class: "hint", "現在 {snap.clients} 台が画面を受信中" }
            }

            section { class: "card",
                h3 { "Windows 起動時に待機" }
                label { class: "check-row",
                    input {
                        r#type: "checkbox",
                        checked: startup_on(),
                        onchange: move |e| {
                            let on = e.checked();
                            match startup::set(on) {
                                Ok(()) => startup_on.set(on),
                                Err(err) => settings_msg.set(format!("登録に失敗: {err}")),
                            }
                        },
                    }
                    "ログイン時に Shut をホストとして自動起動する"
                }
                if !settings_msg().is_empty() {
                    p { class: "error", "{settings_msg}" }
                }
            }

            if snap.password_set {
                section { class: "card",
                    h3 { "パスワード" }
                    p { class: "hint", "設定済み — 変更する場合は新しいパスワードを入力" }
                    div { class: "input-row",
                        input {
                            r#type: "password",
                            placeholder: "新しいパスワード",
                            value: "{pw_input}",
                            oninput: move |e| pw_input.set(e.value()),
                            onkeydown: {
                                let mut f = pw_submit!();
                                move |e| {
                                    if e.key() == Key::Enter {
                                        f(())
                                    }
                                }
                            },
                        }
                        button { class: "quiet", onclick: pw_submit!(), "変更" }
                    }
                    if !pw_msg().is_empty() {
                        p { class: "notice", "{pw_msg}" }
                    }
                }
            }

            section { class: "card",
                h3 { "スケジュールシャットダウン" }
                {match &snap.phase {
                    Phase::Config => rsx! {
                        p { class: "hint", "監視していません" }
                        button {
                            class: "primary",
                            onclick: {
                                let state = state.clone();
                                move |_| {
                                    let _ = state.lock().unwrap().arm();
                                }
                            },
                            "監視を開始"
                        }
                    },
                    Phase::Armed { .. } => rsx! {
                        p { class: "hint", "監視中 — 条件を満たすとシャットダウンします" }
                        button {
                            class: "quiet",
                            onclick: {
                                let state = state.clone();
                                move |_| {
                                    state.lock().unwrap().phase = Phase::Config;
                                }
                            },
                            "監視を解除"
                        }
                    },
                    Phase::Countdown { deadline, reason } => {
                        let left = deadline.saturating_duration_since(std::time::Instant::now()).as_secs();
                        rsx! {
                            p { class: "warn-big", "あと {left} 秒でシャットダウン ({reason})" }
                            button {
                                class: "primary",
                                onclick: {
                                    let state = state.clone();
                                    move |_| {
                                        let _ = crate::power::abort();
                                        state.lock().unwrap().phase = Phase::Config;
                                    }
                                },
                                "シャットダウンを中止"
                            }
                        }
                    }
                    Phase::Done { message } => rsx! {
                        p { class: "hint", "{message}" }
                    },
                }}
                p { class: "hint",
                    "カウントダウン {snap.countdown_secs} 秒"
                    {snap.fw_ago.map(|a| format!(" / 最終ログ更新 {} 秒前", a)).unwrap_or_default()}
                }
            }

            section { class: "card",
                h3 { "外部公開 (cloudflared)" }
                {match &snap.tunnel {
                    TunnelState::Off => rsx! {
                        button {
                            class: "quiet",
                            onclick: {
                                let state = state.clone();
                                move |_| {
                                    if let Some(i) = info.read().as_ref()
                                        && let Ok(i) = i {
                                        ops::start_tunnel(&state, i.port);
                                    }
                                }
                            },
                            "トンネルを開始"
                        }
                    },
                    TunnelState::Starting => rsx! { p { class: "hint", "開始しています…" } },
                    TunnelState::Running(u) => rsx! {
                        p { class: "hint mono", "{u}" }
                        button {
                            class: "quiet",
                            onclick: {
                                let state = state.clone();
                                move |_| ops::stop_tunnel(&state)
                            },
                            "トンネルを停止"
                        }
                    },
                    TunnelState::Failed(e) => rsx! {
                        p { class: "error", "{e}" }
                        button {
                            class: "quiet",
                            onclick: {
                                let state = state.clone();
                                move |_| ops::stop_tunnel(&state)
                            },
                            "閉じる"
                        }
                    },
                }}
            }
        }
    }
}

struct Snap {
    phase: Phase,
    password_set: bool,
    mirror: bool,
    control: bool,
    clients: usize,
    tunnel: TunnelState,
    force_close: bool,
    countdown_secs: u32,
    fw_ago: Option<u64>,
}

fn qr_data_url(text: &str) -> Option<String> {
    use base64::Engine;
    let code = qrcode::QrCode::new(text.as_bytes()).ok()?;
    let width = code.width();
    let colors = code.to_colors();
    const SCALE: usize = 6;
    const QUIET: usize = 4;
    let dim = (width + QUIET * 2) * SCALE;
    let mut luma = vec![255u8; dim * dim];
    for y in 0..width {
        for x in 0..width {
            if colors[y * width + x] == qrcode::Color::Dark {
                for yy in 0..SCALE {
                    let row = (y + QUIET) * SCALE + yy;
                    luma[row * dim + (x + QUIET) * SCALE..][..SCALE].fill(0);
                }
            }
        }
    }
    use image::ImageEncoder;
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(&luma, dim as u32, dim as u32, image::ColorType::L8)
        .ok()?;
    Some(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(png)
    ))
}

// ---------- クライアント画面 ----------

fn recents_path() -> Option<std::path::PathBuf> {
    dirs::data_local_dir().map(|d| d.join("Shut").join("recents.json"))
}

fn load_recents() -> Vec<String> {
    recents_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_recents(list: &[String]) {
    if let Some(p) = recents_path() {
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(p, serde_json::to_string(list).unwrap_or_default());
    }
}

fn push_recent(list: Vec<String>, url: &str) -> Vec<String> {
    let mut list: Vec<String> = list.into_iter().filter(|u| u != url).collect();
    list.push(url.to_string());
    while list.len() > 8 {
        list.remove(0);
    }
    save_recents(&list);
    list
}

#[component]
fn Client(on_back: EventHandler) -> Element {
    let mut input = use_signal(String::new);
    let mut password = use_signal(String::new);
    let mut message = use_signal(String::new);
    let mut recents = use_signal(load_recents);
    let mut connecting = use_signal(|| None::<Receiver<Result<Remote, String>>>);
    let mut remote = use_signal(|| None::<Remote>);

    use_future(move || async move {
        loop {
            let result = connecting
                .read()
                .as_ref()
                .and_then(|rx| rx.try_recv().ok());
            if let Some(result) = result {
                connecting.set(None);
                match result {
                    Ok(r) => {
                        message.set(String::new());
                        recents.set(push_recent(recents(), &input()));
                        remote.set(Some(r));
                    }
                    Err(e) => message.set(e),
                }
            }
            tokio::time::sleep(Duration::from_millis(120)).await;
        }
    });

    let mut start_connect = move |url: String, pw: String| {
        match validate_connection(&url) {
            Ok(target) => {
                input.set(url);
                message.set("接続しています…".into());
                connecting.set(Some(clientnet::connect(
                    target.base_url(),
                    target.token(),
                    pw,
                )));
            }
            Err(error) => message.set(error.into()),
        }
    };

    // `shut <url>` / SHUT_REMOTE_URL で自動接続
    let mut start_connect2 = start_connect;
    use_hook(move || {
        if let Some(url) = auto_url() {
            let pw = std::env::var("SHUT_REMOTE_PASSWORD").unwrap_or_default();
            input.set(url.clone());
            start_connect2(url, pw);
        }
    });

    if let Some(session) = remote() {
        return rsx! {
            Session {
                remote: session,
                on_disconnect: move |_| remote.set(None),
            }
        };
    }

    rsx! {
        main { class: "shell",
            header { class: "topbar",
                div { class: "brand", span { class: "brand-dot" } "Shut" }
                span { class: "pill", "クライアント" }
                button { class: "quiet back", onclick: move |_| on_back.call(()), "← メニュー" }
            }
            section { class: "intro",
                h1 { "もう一台の、自分の机。" }
                p { "ホスト側のPCに表示されたアドレスとパスワードを入力してください。" }
            }
            section { class: "card connect-card",
                label { r#for: "connection-url", "接続アドレス" }
                div { class: "input-row",
                    input {
                        id: "connection-url",
                        r#type: "text",
                        placeholder: "http://192.168.x.x:47823",
                        autocomplete: "off",
                        spellcheck: "false",
                        autocapitalize: "none",
                        value: "{input}",
                        oninput: move |e| input.set(e.value()),
                        onkeydown: move |e| {
                            if e.key() == Key::Enter {
                                start_connect(input(), password());
                            }
                        },
                    }
                }
                div { class: "input-row", style: "margin-top:10px",
                    input {
                        id: "connection-password",
                        r#type: "password",
                        placeholder: "パスワード",
                        value: "{password}",
                        oninput: move |e| password.set(e.value()),
                        onkeydown: move |e| {
                            if e.key() == Key::Enter {
                                start_connect(input(), password());
                            }
                        },
                    }
                    button {
                        class: "primary",
                        disabled: connecting.read().is_some(),
                        onclick: move |_| start_connect(input(), password()),
                        {if connecting.read().is_some() { "接続中…" } else { "接続" }}
                    }
                }
                if !message().is_empty() {
                    p { class: "notice", "{message}" }
                }
            }

            if !recents().is_empty() {
                section { class: "card recent-card",
                    div { class: "recent-head",
                        span { "最近の接続" }
                        button {
                            class: "quiet",
                            onclick: move |_| {
                                save_recents(&[]);
                                recents.set(vec![]);
                            },
                            "消去"
                        }
                    }
                    for url in recents() {
                        button {
                            class: "recent-row",
                            onclick: move |_| {
                                input.set(url.clone());
                                start_connect(url.clone(), password());
                            },
                            span { class: "recent-endpoint", "{url}" }
                            span { class: "recent-action", "再接続 →" }
                        }
                    }
                }
            }
        }
    }
}

// ---------- セッション ----------

#[component]
fn Session(remote: Remote, on_disconnect: EventHandler) -> Element {
    let mut compose = use_signal(String::new);
    let mut log = use_signal(String::new);
    let mut last_clip = use_signal(|| None::<String>);
    let mut view_size = use_signal(|| (1160.0f64, 597.0f64));
    let mut last_pos = use_signal(|| (0.5f64, 0.5f64));
    let remote = use_signal(|| remote);
    let endpoint = remote.read().endpoint.clone();

    // ワーカーのメッセージとクリップボード返信を拾う
    let shared = remote.read().shared.clone();
    use_future(move || {
        let shared = shared.clone();
        async move {
            loop {
                if let Some(msg) = shared.messages.lock().unwrap().pop_front() {
                    log.set(msg);
                }
                if let Some(cb) = shared.clipboard.lock().unwrap().take() {
                    last_clip.set(Some(cb.clone()));
                    let _ = document::eval(&format!(
                        "navigator.clipboard.writeText({})",
                        serde_json::to_string(&cb).unwrap()
                    ));
                    log.set("PC側クリップボードを取り込みました".into());
                }
                tokio::time::sleep(Duration::from_millis(120)).await;
            }
        }
    });

    let normalized = move |e: &PointerEvent| {
        let pt = e.element_coordinates();
        let (w, h) = view_size();
        (
            (pt.x / w.max(1.0)).clamp(0.0, 1.0),
            (pt.y / h.max(1.0)).clamp(0.0, 1.0),
        )
    };

    rsx! {
        div {
            id: "session-root",
            class: "session",
            tabindex: -1,
            oncontextmenu: move |e| e.prevent_default(),
            onmounted: move |e| async move {
                let _ = e.set_focus(true).await;
                // H.264 -> Rust側デコード -> ローカルWS(framesrv)で
                // 最新RGBAフレームをpush受信してcanvasへletterbox描画。
                let port = framesrv::port();
                document::eval(&format!(
                    r#"(()=>{{
const cvs=document.getElementById('vc');
if(!cvs)return 'no canvas';
const ctx=cvs.getContext('2d');
const off=new OffscreenCanvas(16,16);
const octx=off.getContext('2d');
let err='ws connecting';
function drawErr(){{
  const w=cvs.width,h=cvs.height;
  if(!err)return;
  ctx.fillStyle='#141712';ctx.fillRect(0,0,w,h);
  ctx.fillStyle='#8ba';ctx.font='14px monospace';
  ctx.fillText('video: '+err,16,26);
}}
function connect(){{
  const ws=new WebSocket('ws://127.0.0.1:{port}');
  ws.binaryType='arraybuffer';
  ws.onmessage=(ev)=>{{
    const b=ev.data;
    if(b.byteLength<16)return;
    const dv=new DataView(b);
    const fw=dv.getUint32(0,true),fh=dv.getUint32(4,true);
    const w=cvs.clientWidth|0,h=cvs.clientHeight|0;
    if(w&&h&&(cvs.width!==w||cvs.height!==h)){{cvs.width=w;cvs.height=h;}}
    if(off.width!==fw||off.height!==fh){{off.width=fw;off.height=fh;}}
    octx.putImageData(new ImageData(new Uint8ClampedArray(b,16),fw,fh),0,0);
    const s=Math.min(w/fw,h/fh),dw=fw*s,dh=fh*s;
    ctx.fillStyle='#141712';ctx.fillRect(0,0,w,h);
    ctx.drawImage(off,(w-dw)/2,(h-dh)/2,dw,dh);
    err='';
  }};
  ws.onopen=()=>{{err='';}};
  ws.onerror=()=>{{err='ws error';drawErr();}};
  ws.onclose=()=>{{err='ws closed';drawErr();
    if(document.getElementById('vc'))setTimeout(connect,500);}};
}}
connect();
return 'ok';}})()"#
                ));
            },
            onkeydown: move |e| {
                e.prevent_default();
                let key = e.key().to_string();
                let mods = e.modifiers();
                let mut mods_list = Vec::new();
                if mods.contains(Modifiers::CONTROL) {
                    mods_list.push("Control".to_string());
                }
                if mods.contains(Modifiers::ALT) {
                    mods_list.push("Alt".to_string());
                }
                if mods.contains(Modifiers::SHIFT) {
                    mods_list.push("Shift".to_string());
                }
                if matches!(
                    key.as_str(),
                    "Shift" | "Control" | "Alt" | "Meta" | "CapsLock" | "Process"
                        | "Unidentified" | "Dead" | "AltGraph" | "NumLock"
                ) {
                    return;
                }
                let name = match key.as_str() {
                    "ArrowUp" => "Up",
                    "ArrowDown" => "Down",
                    "ArrowLeft" => "Left",
                    "ArrowRight" => "Right",
                    " " => "Space",
                    k => k,
                };
                let special = matches!(
                    name,
                    "Enter" | "Backspace" | "Escape" | "Tab" | "Delete" | "Home" | "End"
                        | "PageUp" | "PageDown" | "Space" | "Up" | "Down" | "Left"
                        | "Right"
                ) || name
                    .strip_prefix('F')
                    .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
                let has_command_mod = mods_list.iter().any(|m| m == "Control" || m == "Alt");
                if has_command_mod || special {
                    remote.read().input(InputEvent::key(name, mods_list));
                } else if key.chars().count() == 1 {
                    remote.read().input(InputEvent::text(key));
                }
            },

            div {
                class: "stage",
                onmounted: move |e| async move {
                    if let Ok(rect) = e.get_client_rect().await {
                        view_size.set((rect.size.width, rect.size.height));
                    }
                },
                onpointermove: move |e| {
                    let (x, y) = normalized(&e);
                    last_pos.set((x, y));
                    remote.read().input(InputEvent::pointer("move", x, y));
                },
                onpointerdown: move |e| {
                    let (x, y) = normalized(&e);
                    last_pos.set((x, y));
                    let button = match e.trigger_button() {
                        Some(MouseButton::Secondary) => "right",
                        _ => "left",
                    };
                    let mut ev = InputEvent::pointer("down", x, y);
                    ev.button = button.into();
                    remote.read().input(ev);
                },
                onpointerup: move |e| {
                    let (x, y) = normalized(&e);
                    last_pos.set((x, y));
                    let button = match e.trigger_button() {
                        Some(MouseButton::Secondary) => "right",
                        _ => "left",
                    };
                    let mut ev = InputEvent::pointer("up", x, y);
                    ev.button = button.into();
                    remote.read().input(ev);
                },
                ondoubleclick: move |e| {
                    let pt = e.element_coordinates();
                    let (w, h) = view_size();
                    let mut ev = InputEvent::pointer(
                        "double_click",
                        (pt.x / w.max(1.0)).clamp(0.0, 1.0),
                        (pt.y / h.max(1.0)).clamp(0.0, 1.0),
                    );
                    ev.button = "left".into();
                    remote.read().input(ev);
                },
                onwheel: move |e| {
                    let (x, y) = last_pos();
                    let dy = match e.delta() {
                        WheelDelta::Pixels(v) => v.y / 120.0,
                        WheelDelta::Lines(v) => v.y,
                        WheelDelta::Pages(v) => v.y * 8.0,
                    };
                    let mut ev = InputEvent::pointer("scroll", x, y);
                    ev.dy = dy;
                    remote.read().input(ev);
                },
                canvas { id: "vc", class: "stream" }
            }

            header { class: "session-bar",
                div { class: "brand small", span { class: "brand-dot" } "Shut" }
                span { class: "endpoint", "{endpoint} | {log}" }
                button {
                    class: "quiet",
                    onclick: move |_| {
                        document::eval("document.getElementById('session-root')?.focus()");
                        on_disconnect.call(());
                    },
                    "切断"
                }
            }

            footer { class: "dock",
                div { class: "dock-row",
                    input {
                        class: "compose",
                        r#type: "text",
                        placeholder: "日本語・長文はここに入力して送信",
                        value: "{compose}",
                        oninput: move |e| compose.set(e.value()),
                        onkeydown: move |e| {
                            e.stop_propagation();
                            if e.key() == Key::Enter {
                                let text = compose();
                                if !text.is_empty() {
                                    remote.read().input(InputEvent::text(text));
                                    compose.set(String::new());
                                }
                            }
                        },
                    }
                    button {
                        class: "primary",
                        onclick: move |_| {
                            let text = compose();
                            if !text.is_empty() {
                                remote.read().input(InputEvent::text(text));
                                compose.set(String::new());
                            }
                            document::eval("document.getElementById('session-root')?.focus()");
                        },
                        "送信"
                    }
                }
                div { class: "dock-row tools",
                    button {
                        class: "quiet",
                        onclick: move |_| remote.read().send(Job::ClipboardGet),
                        "PCコピーを取込"
                    }
                    button {
                        class: "quiet",
                        onclick: move |_| remote.read().send(Job::ClipboardSet(compose())),
                        "コピーへ送る"
                    }
                    button {
                        class: "quiet",
                        onclick: move |_| {
                            if let Some(path) = rfd::FileDialog::new().pick_file()
                                && let Ok(bytes) = std::fs::read(&path)
                            {
                                let name = path
                                    .file_name()
                                    .map(|n| n.to_string_lossy().into_owned())
                                    .unwrap_or_else(|| "file.bin".into());
                                remote.read().send(Job::Upload { name, bytes });
                            }
                        },
                        "ファイルを送る"
                    }
                    span { class: "spacer" }
                    button {
                        class: "quiet",
                        onclick: move |_| remote.read().send(Job::Arm),
                        "監視を開始"
                    }
                    button {
                        class: "quiet",
                        onclick: move |_| remote.read().send(Job::Disarm),
                        "監視を解除"
                    }
                    button {
                        class: "quiet warn",
                        onclick: move |_| remote.read().send(Job::Shutdown { now: true }),
                        "今すぐシャットダウン"
                    }
                    button {
                        class: "quiet",
                        onclick: move |_| remote.read().send(Job::Cancel),
                        "中止"
                    }
                }
                div { class: "dock-row status",
                    span { class: "hint", "文字キーは直接PCへ。日本語は上の入力欄から送信します。" }
                    span { class: "status-line", "{log}" }
                }
            }
        }
    }
}

// ---------- atelier スタイル ----------

const CSS: &str = r#"
:root {
    --paper: #f5f7ef;
    --ink: #29312d;
    --muted: #69736b;
    --accent: #2d9760;
    --line: #c1cabc;
    --card: rgba(252, 254, 247, 0.82);
    --warn: #9c3b30;
}
* { box-sizing: border-box; margin: 0; }
html, body, #main { height: 100%; }
body {
    background: var(--paper);
    color: var(--ink);
    font-family: "Segoe UI", "Yu Gothic UI", "Hiragino Kaku Gothic ProN", sans-serif;
    line-height: 1.65;
}
body::before {
    content: "";
    position: fixed; inset: 0; z-index: 0; pointer-events: none;
    background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='240' height='240'%3E%3Cfilter id='n'%3E%3CfeTurbulence type='fractalNoise' baseFrequency='0.85' numOctaves='2'/%3E%3C/filter%3E%3Crect width='240' height='240' filter='url(%23n)' opacity='0.05'/%3E%3C/svg%3E");
}
body::after {
    content: "";
    position: fixed; inset: 0; z-index: 0; pointer-events: none;
    background:
        radial-gradient(38% 30% at 12% 18%, rgba(122, 164, 200, 0.16), transparent 70%),
        radial-gradient(30% 26% at 88% 12%, rgba(214, 148, 158, 0.14), transparent 70%),
        radial-gradient(46% 34% at 78% 88%, rgba(45, 151, 96, 0.13), transparent 70%);
}

.shell {
    position: relative; z-index: 1;
    max-width: 780px; margin: 0 auto;
    min-height: 100%; padding: 48px 28px 40px;
    display: flex; flex-direction: column; gap: 18px;
    overflow-y: auto;
}
.topbar { display: flex; align-items: center; gap: 12px; }
.brand { display: flex; align-items: center; gap: 9px; font-size: 21px; font-weight: 700; letter-spacing: 0.04em; }
.brand.small { font-size: 15px; }
.brand-dot {
    width: 11px; height: 11px; border-radius: 42% 58% 51% 49%;
    background: var(--accent); box-shadow: 0 0 0 3px rgba(45, 151, 96, 0.16);
}
.pill {
    font-size: 11px; letter-spacing: 0.14em; text-transform: uppercase;
    color: var(--muted); border: 1px solid var(--line);
    border-radius: 999px; padding: 3px 12px;
}
.back { margin-left: auto; font-size: 12.5px; padding: 5px 14px; }
.intro h1 { font-size: 30px; font-weight: 700; letter-spacing: 0.02em; }
.intro p { color: var(--muted); margin-top: 6px; max-width: 46em; }
.card {
    background: var(--card);
    border: 1px solid rgba(255, 255, 255, 0.75);
    outline: 1px solid var(--line);
    border-radius: 16px;
    padding: 20px 24px;
    box-shadow: 0 14px 34px -22px rgba(105, 115, 107, 0.55);
}
.card h3 { font-size: 15px; margin-bottom: 10px; letter-spacing: 0.02em; }
.warn-card { outline: 2px solid rgba(156, 59, 48, 0.5); transform: rotate(-0.3deg); }
.connect-card { transform: rotate(-0.35deg); }
.card label, .connect-card label {
    display: block; font-size: 12px; letter-spacing: 0.1em;
    text-transform: uppercase; color: var(--muted); margin-bottom: 10px;
}
.input-row { display: flex; gap: 10px; }
input[type="text"], input[type="password"], input:not([type]) {
    flex: 1; min-width: 0;
    background: rgba(255, 255, 255, 0.7);
    border: 1px solid var(--line); border-radius: 11px;
    padding: 11px 14px; font-size: 15px; color: var(--ink);
    font-family: inherit;
}
input[type="checkbox"] { width: 17px; height: 17px; accent-color: var(--accent); }
input:focus { outline: 2px solid rgba(45, 151, 96, 0.4); outline-offset: 1px; border-color: var(--accent); }
button {
    font-family: inherit; font-size: 14px; cursor: pointer;
    border-radius: 11px; border: 1px solid transparent;
    padding: 10px 18px; transition: all 0.25s ease;
}
button:disabled { opacity: 0.55; cursor: default; }
.primary { background: var(--accent); color: #fdfef8; font-weight: 600; }
.primary:hover:not(:disabled) { filter: brightness(1.06); transform: translateY(-1px); }
.quiet {
    background: transparent; color: var(--ink);
    border-color: var(--line);
}
.quiet:hover { background: rgba(41, 49, 45, 0.05); }
.quiet.warn { color: var(--warn); border-color: rgba(156, 59, 48, 0.35); }
.quiet.warn:hover { background: rgba(156, 59, 48, 0.07); }
.notice {
    margin-top: 14px; font-size: 13.5px; color: var(--muted);
    border-left: 3px solid var(--accent); padding-left: 12px;
}
.error { color: var(--warn); font-size: 13.5px; margin-top: 10px; }
.hint { color: var(--muted); font-size: 12.5px; }
.mono { font-family: Consolas, monospace; word-break: break-all; }
.check-row {
    display: flex; align-items: center; gap: 10px;
    font-size: 14.5px; padding: 5px 0; cursor: pointer;
}
.warn-big { color: var(--warn); font-weight: 700; font-size: 17px; margin: 8px 0; }
.big-url {
    font-family: Consolas, monospace; font-size: 19px; letter-spacing: 0.02em;
    background: rgba(255,255,255,0.75); border: 1px solid var(--line);
    padding: 10px 16px; border-radius: 11px;
}
.url-row { display: flex; align-items: center; gap: 18px; flex-wrap: wrap; }
.qr {
    width: 148px; height: 148px; image-rendering: pixelated;
    border: 1px solid var(--line); border-radius: 10px; background: #fff;
}
.host-url-card { transform: rotate(0.25deg); }

/* モード選択 */
.menu-shell { justify-content: flex-start; }
.menu-cards { display: grid; grid-template-columns: 1fr 1fr; gap: 22px; margin-top: 8px; }
.menu-card {
    text-align: left; display: flex; flex-direction: column; gap: 10px;
    background: var(--card);
    border: 1px solid rgba(255,255,255,0.75);
    outline: 1px solid var(--line);
    border-radius: 18px; padding: 26px 24px;
    box-shadow: 0 16px 38px -24px rgba(105,115,107,0.6);
    transform: rotate(-0.5deg);
    transition: transform 0.25s ease, box-shadow 0.25s ease;
    font-family: inherit; color: var(--ink);
}
.menu-card.alt { transform: rotate(0.45deg); }
.menu-card:hover {
    transform: rotate(0deg) translateY(-3px);
    box-shadow: 0 22px 44px -22px rgba(105,115,107,0.65);
}
.menu-icon {
    width: 44px; height: 44px; border-radius: 12px;
    display: grid; place-items: center;
    background: rgba(45,151,96,0.12); color: var(--accent);
    font-size: 22px;
}
.menu-card h2 { font-size: 19px; }
.menu-card p { color: var(--muted); font-size: 13.5px; }
.menu-go { color: var(--accent); font-weight: 600; font-size: 14px; margin-top: auto; }
.foot {
    margin-top: auto; display: flex; align-items: center; gap: 8px;
    font-size: 12.5px; color: var(--muted);
}
.privacy-dot { width: 7px; height: 7px; border-radius: 50%; background: var(--accent); opacity: 0.7; }
.recent-card { transform: rotate(0.3deg); }
.recent-head {
    display: flex; justify-content: space-between; align-items: center;
    font-size: 12px; letter-spacing: 0.1em; text-transform: uppercase;
    color: var(--muted); margin-bottom: 12px;
}
.recent-head .quiet { padding: 4px 12px; font-size: 12px; }
.recent-row {
    display: flex; justify-content: space-between; align-items: center;
    width: 100%; text-align: left; gap: 16px;
    background: transparent; border: none; border-top: 1px dashed var(--line);
    padding: 12px 4px; border-radius: 0;
}
.recent-row:hover { background: rgba(45, 151, 96, 0.06); }
.recent-endpoint { font-size: 14px; font-family: Consolas, monospace; }
.recent-action { font-size: 12.5px; color: var(--accent); white-space: nowrap; }

/* セッション */
.session {
    position: fixed; inset: 0; z-index: 1; outline: none;
    display: flex; flex-direction: column;
    background: #232824;
}
.stage {
    flex: 1; min-height: 0; position: relative;
    touch-action: none; overflow: hidden;
}
.stream {
    position: absolute; inset: 0;
    width: 100%; height: 100%;
    user-select: none; -webkit-user-select: none;
    cursor: crosshair;
}
.session-bar {
    position: absolute; top: 0; left: 0; right: 0; z-index: 10;
    display: flex; align-items: center; gap: 12px;
    padding: 10px 18px;
    background: linear-gradient(rgba(245, 247, 239, 0.92), rgba(245, 247, 239, 0));
    color: var(--ink);
    pointer-events: none;
}
.session-bar button { pointer-events: auto; }
.endpoint { font-size: 13px; font-family: Consolas, monospace; color: var(--muted); }
.session-bar .quiet {
    margin-left: auto; font-size: 12.5px; padding: 5px 14px;
    background: rgba(252, 254, 247, 0.85);
}
.dock {
    flex: 0 0 auto; z-index: 10;
    margin: 0 auto 14px;
    width: min(860px, calc(100% - 32px));
    display: flex; flex-direction: column; gap: 8px;
    background: var(--card);
    border: 1px solid rgba(255, 255, 255, 0.75);
    outline: 1px solid var(--line);
    border-radius: 15px; padding: 12px 16px;
    box-shadow: 0 16px 40px -24px rgba(35, 40, 36, 0.7);
}
.dock-row { display: flex; align-items: center; gap: 9px; }
.dock-row.tools { flex-wrap: wrap; }
.dock-row.tools .quiet { font-size: 12.5px; padding: 6px 13px; }
.dock .compose { font-size: 14px; padding: 9px 13px; }
.dock .primary { padding: 9px 20px; }
.spacer { flex: 1; }
.dock-row.status { justify-content: space-between; font-size: 12px; color: var(--muted); }
.status-line { font-style: italic; }
"#;
