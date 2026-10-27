mod net;

use std::sync::mpsc::Receiver;
use std::time::Duration;

use dioxus::html::geometry::WheelDelta;
use dioxus::html::input_data::MouseButton;
use dioxus::prelude::*;
use net::{Job, Remote};
use shut_protocol::{InputEvent, validate_connection};

fn main() {
    // 検証用: SHUT_WINDOW_POS="x,y[,w,h]" で初期位置・サイズを指定
    // （起動時は非表示のまま配置され、初回描画後にその場で表示される）
    let pos: Vec<f64> = std::env::var("SHUT_WINDOW_POS")
        .ok()
        .map(|s| s.split(',').filter_map(|p| p.trim().parse().ok()).collect())
        .unwrap_or_default();
    let mut wb = dioxus::desktop::WindowBuilder::new()
        .with_title("Shut Remote")
        .with_inner_size(dioxus::desktop::LogicalSize::new(1160.0, 760.0));
    if pos.len() >= 2 {
        wb = wb.with_position(dioxus::desktop::LogicalPosition::new(pos[0], pos[1]));
        if pos.len() >= 4 {
            wb = wb.with_inner_size(dioxus::desktop::LogicalSize::new(pos[2], pos[3]));
        }
    }
    dioxus::LaunchBuilder::desktop()
        .with_cfg(dioxus::desktop::Config::new().with_window(wb))
        .launch(App);
}

// ---------- 直近の接続履歴 ----------

fn recents_path() -> Option<std::path::PathBuf> {
    dirs::data_local_dir().map(|d| d.join("ShutRemote").join("recents.json"))
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

// ---------- アプリ ----------

#[component]
fn App() -> Element {
    let mut input = use_signal(String::new);
    let mut message = use_signal(String::new);
    let mut recents = use_signal(load_recents);
    let mut connecting = use_signal(|| None::<Receiver<Result<Remote, String>>>);
    let mut remote = use_signal(|| None::<Remote>);

    // 接続試行の結果を拾う
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
                        let url = input();
                        recents.set(push_recent(recents(), &url));
                        remote.set(Some(r));
                    }
                    Err(e) => message.set(e),
                }
            }
            tokio::time::sleep(Duration::from_millis(120)).await;
        }
    });

    let mut start_connect = move |url: String| match validate_connection(&url) {
        Ok(target) => {
            input.set(url.clone());
            message.set("接続しています…".into());
            connecting.set(Some(net::connect(target.base_url(), target.token())));
        }
        Err(error) => message.set(error.into()),
    };

    // `shut-remote <url>` / SHUT_REMOTE_URL で起動時に自動接続する
    use_hook(move || {
        let auto = std::env::var("SHUT_REMOTE_URL")
            .ok()
            .or_else(|| std::env::args().nth(1));
        if let Some(url) = auto {
            input.set(url.clone());
            start_connect(url);
        }
    });


    if let Some(session) = remote() {
        return rsx! {
            style { dangerous_inner_html: CSS }
            Session {
                remote: session,
                on_disconnect: move |_| remote.set(None),
            }
        };
    }

    rsx! {
        style { dangerous_inner_html: CSS }
        document::Title { "Shut Remote" }
        main { class: "shell",
            header { class: "topbar",
                div { class: "brand", span { class: "brand-dot" } "Shut" }
                span { class: "pill", "リモートデスクトップ" }
            }
            section { class: "intro",
                h1 { "もう一台の、自分の机。" }
                p { "PCのShutに表示された接続URLを貼るだけ。画面とキーボードが、そのまま手元に届きます。" }
            }
            section { class: "card connect-card",
                label { r#for: "connection-url", "接続URL" }
                div { class: "input-row",
                    input {
                        id: "connection-url",
                        r#type: "text",
                        placeholder: "https://…/?t=…",
                        autocomplete: "off",
                        spellcheck: "false",
                        autocapitalize: "none",
                        oninput: move |e| {
                            input.set(e.value());
                            message.set(String::new());
                        },
                    }
                    button {
                        class: "primary",
                        disabled: connecting.read().is_some(),
                        onclick: move |_| start_connect(input()),
                        if connecting.read().is_some() { "接続中…" } else { "接続" }
                    }
                }
                if !message().is_empty() {
                    div { class: "notice", role: "status", "{message}" }
                }
            }
            if !recents().is_empty() {
                section { class: "card recent-card",
                    div { class: "recent-head",
                        span { "前回つないだPC" }
                        button {
                            class: "quiet",
                            onclick: move |_| {
                                save_recents(&[]);
                                recents.set(Vec::new());
                            },
                            "消す"
                        }
                    }
                    for url in recents().iter().rev() {
                        {
                            let url = url.clone();
                            rsx! {
                                button {
                                    class: "recent-row",
                                    onclick: move |_| {
                                        input.set(url.clone());
                                        start_connect(url.clone());
                                    },
                                    span { class: "recent-endpoint",
                                        match validate_connection(&url) {
                                            Ok(t) => t.endpoint.clone(),
                                            Err(_) => url.clone(),
                                        }
                                    }
                                    span { class: "recent-action", "もう一度つなぐ" }
                                }
                            }
                        }
                    }
                }
            }
            footer { class: "foot",
                span { class: "privacy-dot" }
                "接続URLはPCの鍵です。貼り先には気をつけて。"
            }
        }
    }
}

// ---------- セッション画面 ----------

#[component]
fn Session(remote: Remote, on_disconnect: EventHandler<()>) -> Element {
    // Signalで包んで全ハンドラからCopyアクセスできるようにする
    let remote = use_signal(|| remote);
    let shared = remote.read().shared.clone();
    let mut view_size = use_signal(|| (1.0f64, 1.0f64));
    let mut last_pos = use_signal(|| (0.5f64, 0.5f64));
    let mut compose = use_signal(String::new);
    let mut log = use_signal(String::new);

    // ワーカーからのメッセージとクリップボード取り込みを拾う
    use_future(move || {
        let shared = shared.clone();
        async move {
        loop {
            {
                let mut q = shared.messages.lock().unwrap();
                if let Some(latest) = q.pop_back() {
                    log.set(latest);
                    q.clear();
                }
            }
            if let Some(text) = shared.clipboard.lock().unwrap().take() {
                compose.set(text);
                log.set("PCのクリップボードを取り込みました".into());
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        }
    });

    let normalized = move |e: &PointerEvent| -> (f64, f64) {
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
                let probe = document::eval(
                    "const c=document.querySelector('#vc');return 'secure='+window.isSecureContext+' vd='+typeof VideoDecoder+' ws='+typeof WebSocket+' fail='+(c?c.dataset.failed:'-')+' loc='+location.href.slice(0,40);",
                );
                let result = probe.await;
                let _ = std::fs::write(
                    r"C:\Users\yamau\Desktop\Shut\docs\verification\dom-probe.txt",
                    format!("{result:?}"),
                );
                // MJPEGをcanvasへレターボックス描画。
                // dioxus WebViewはsecure contextでなくWebCodecs不可のため、
                // img要素ではなく非表示Image+canvas経由にして
                // replaced-elementのコンポジットはみ出しを回避する。
                let mjpeg_url = serde_json::to_string(&remote.read().stream_url).unwrap();
                document::eval(&format!(
                    r#"(()=>{{
const cvs=document.getElementById('vc');
if(!cvs)return 'no canvas';
const ctx=cvs.getContext('2d');
const src=new Image();
src.src={mjpeg_url};
function draw(){{
  const w=cvs.clientWidth|0,h=cvs.clientHeight|0;
  if(!w||!h){{requestAnimationFrame(draw);return;}}
  if(cvs.width!==w||cvs.height!==h){{cvs.width=w;cvs.height=h;}}
  ctx.fillStyle='#141712';ctx.fillRect(0,0,w,h);
  if(src.naturalWidth>0){{
    const s=Math.min(w/src.naturalWidth,h/src.naturalHeight);
    const dw=src.naturalWidth*s,dh=src.naturalHeight*s;
    ctx.drawImage(src,(w-dw)/2,(h-dh)/2,dw,dh);
  }}
  requestAnimationFrame(draw);
}}
requestAnimationFrame(draw);
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
                // IME処理中・修飾キー単体・未知キーは送らない
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
                    // レイアウトやホスト側IMEに左右されないよう文字はUnicode注入で送る
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
                span { class: "endpoint", "{remote.read().endpoint} | {log}" }
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

/* ---------- 接続画面 ---------- */
.shell {
    position: relative; z-index: 1;
    max-width: 760px; margin: 0 auto;
    min-height: 100%; padding: 56px 28px 40px;
    display: flex; flex-direction: column; gap: 26px;
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
.intro h1 { font-size: 30px; font-weight: 700; letter-spacing: 0.02em; }
.intro p { color: var(--muted); margin-top: 6px; max-width: 46em; }
.card {
    background: var(--card);
    border: 1px solid rgba(255, 255, 255, 0.75);
    outline: 1px solid var(--line);
    border-radius: 16px;
    padding: 22px 24px;
    box-shadow: 0 14px 34px -22px rgba(105, 115, 107, 0.55);
}
.connect-card { transform: rotate(-0.35deg); }
.connect-card label, .dock-row label {
    display: block; font-size: 12px; letter-spacing: 0.1em;
    text-transform: uppercase; color: var(--muted); margin-bottom: 10px;
}
.input-row { display: flex; gap: 10px; }
input[type="text"], input:not([type]) {
    flex: 1; min-width: 0;
    background: rgba(255, 255, 255, 0.7);
    border: 1px solid var(--line); border-radius: 11px;
    padding: 11px 14px; font-size: 15px; color: var(--ink);
    font-family: inherit;
}
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
.quiet.warn { color: #9c3b30; border-color: rgba(156, 59, 48, 0.35); }
.quiet.warn:hover { background: rgba(156, 59, 48, 0.07); }
.notice {
    margin-top: 14px; font-size: 13.5px; color: var(--muted);
    border-left: 3px solid var(--accent); padding-left: 12px;
}
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
.recent-endpoint { font-size: 15px; font-family: Consolas, monospace; }
.recent-action { font-size: 12.5px; color: var(--accent); white-space: nowrap; }
.foot {
    margin-top: auto; display: flex; align-items: center; gap: 8px;
    font-size: 12.5px; color: var(--muted);
}
.privacy-dot { width: 7px; height: 7px; border-radius: 50%; background: var(--accent); opacity: 0.7; }

/* ---------- セッション画面 ---------- */
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
    object-fit: contain;
    user-select: none; -webkit-user-select: none;
    cursor: crosshair;
}
.session-bar {
    position: absolute; top: 0; left: 0; right: 0; z-index: 10;
    display: flex; align-items: center; gap: 12px;
    padding: 10px 18px;
    background: linear-gradient(rgba(245, 247, 239, 0.92), rgba(245, 247, 239, 0));
    color: var(--ink);
}
.endpoint { font-size: 13px; font-family: Consolas, monospace; color: var(--muted); }
.session-bar .quiet {
    margin-left: auto; font-size: 12.5px; padding: 5px 14px;
    background: rgba(252, 254, 247, 0.85);
}
.dock {
    flex: 0 0 auto; z-index: 10;
    transform: translateZ(0);
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
