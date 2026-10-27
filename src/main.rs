#![windows_subsystem = "windows"]

mod filewatch;
mod log;
mod power;
mod remote;
mod server;
mod state;

use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use filewatch::FileWatcher;
use state::{AppState, FwStatus, Phase};

fn data_dir() -> PathBuf {
    let dir = dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Shut");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// 外部トリガーファイル。`shut done` が作成し、評価側が検出したら消す。
pub fn trigger_file() -> PathBuf {
    data_dir().join("trigger.flag")
}

fn config_file() -> PathBuf {
    data_dir().join("config.json")
}

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("done") | Some("trigger") => {
            attach_console();
            match std::fs::write(trigger_file(), b"1") {
                Ok(()) => println!("shut: トリガーを送信しました"),
                Err(e) => eprintln!("shut: トリガーの送信に失敗しました: {e}"),
            }
        }
        Some("-h") | Some("--help") | Some("/?") => {
            attach_console();
            println!(
                "Shut - AIエージェントの終了や時刻指定でPCをシャットダウン\n\
                 \n\
                 使い方:\n\
                 \tshut            GUIを起動\n\
                 \tshut done       起動中のGUIに外部トリガーを送信\n\
                 \tshut --help     このヘルプ"
            );
        }
        _ => run(),
    }
}

fn token() -> String {
    // デバッグ用に環境変数で固定できる
    std::env::var("SHUT_TOKEN").unwrap_or_else(|_| format!("{:032x}", rand::random::<u128>()))
}

/// 空いているポートを探してリスナーを確保する
fn bind_listener() -> (TcpListener, u16) {
    for port in 47823u16..47900 {
        if let Ok(l) = TcpListener::bind(("0.0.0.0", port)) {
            return (l, port);
        }
    }
    TcpListener::bind(("0.0.0.0", 0))
        .map(|l| {
            let p = l.local_addr().unwrap().port();
            (l, p)
        })
        .expect("no free port")
}

fn run() {
    if already_running() {
        attach_console();
        eprintln!("shut: 既に起動しています");
        return;
    }
    let state = state::new_state();
    let token = token();
    let (listener, port) = bind_listener();
    listener.set_nonblocking(true).unwrap();
    let lan_url = remote::lan_ip()
        .map(|ip| format!("http://{ip}:{port}"))
        .unwrap_or_else(|| format!("http://127.0.0.1:{port}"));

    let ctx = Arc::new(server::ServerCtx {
        state: state.clone(),
        token: token.clone(),
        port,
        lan_url,
    });

    // トリガー評価＆シャットダウン実行スレッド
    std::thread::spawn({
        let state = state.clone();
        move || eval_loop(state)
    });

    // HTTPサーバー（tokio）
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(3)
            .enable_all()
            .build()
            .expect("tokio runtime");
        rt.block_on(async move {
            let tl = tokio::net::TcpListener::from_std(listener).unwrap();
            let app = server::router(ctx);
            let _ = axum::serve(tl, app).await;
        });
    });

    open_window(&state, port, &token);
}

/// 1秒間隔でトリガー評価とカウントダウン進行を行う。
/// FileWatcherはこのスレッド専有にし、再帰走査をMutex外で行う
/// （走査中にAPIがロック待ちでハングするのを防ぐ）。
fn eval_loop(state: AppState) {
    let mut fw = FileWatcher::new();
    let mut cur_dirs = String::new();
    let mut was_armed = false;
    let mut last_fw = Instant::now() - Duration::from_secs(3);
    loop {
        std::thread::sleep(Duration::from_secs(1));

        // 設定変更の検知用に必要な値だけ短時間取得
        let (armed, dirs) = {
            let st = state.lock().unwrap();
            (
                matches!(st.phase, Phase::Armed { .. } | Phase::Countdown { .. }),
                st.settings.watch_dirs.clone(),
            )
        };
        if dirs != cur_dirs {
            cur_dirs = dirs;
            fw.set_dirs(&cur_dirs);
            fw.refresh();
        }
        if armed && !was_armed {
            fw.reset();
        }
        was_armed = armed;
        if last_fw.elapsed() >= Duration::from_secs(3) {
            last_fw = Instant::now();
            fw.refresh();
        }

        let mut st = state.lock().unwrap();
        st.fw_status = FwStatus {
            saw_activity: fw.saw_activity,
            ago_secs: fw.since_last_write().map(|d| d.as_secs()),
        };
        match st.phase.clone() {
            Phase::Armed { .. } => {
                if let Some(reason) = st.evaluate() {
                    st.fire(reason);
                }
            }
            Phase::Countdown { deadline, .. } if Instant::now() >= deadline => {
                // Windows側のスケジュール済みシャットダウンを再確認する保険として
                // 即時シャットダウンを発行する（既に進行中なら shutdown.exe が失敗するだけ）。
                match power::shutdown(st.settings.force_close) {
                    Ok(()) => {
                        log::line("shutdown executed");
                        st.phase = Phase::Done {
                            message: "シャットダウンを実行しました".into(),
                        }
                    }
                    Err(e) => {
                        log::line(&format!("shutdown failed: {e}"));
                        st.phase = Phase::Done {
                            message: format!("シャットダウンに失敗しました: {e}"),
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

/// WebView2ウィンドウでUIを開く。閉じられたら終了。
fn open_window(state: &AppState, port: u16, token: &str) {
    use tao::event::{Event, WindowEvent};
    use tao::event_loop::{ControlFlow, EventLoop};
    use tao::window::WindowBuilder;
    use wry::WebViewBuilder;

    let url = format!("http://127.0.0.1:{port}/?t={token}");
    let event_loop = EventLoop::new();
    let window = WindowBuilder::new()
        .with_title("Shut")
        .with_inner_size(tao::dpi::LogicalSize::new(880.0f64, 960.0f64))
        .with_min_inner_size(tao::dpi::LogicalSize::new(420.0f64, 640.0f64))
        .build(&event_loop)
        .expect("window");

    let _webview = WebViewBuilder::new()
        .with_url(&url)
        .build(&window)
        .expect("webview");

    let state = state.clone();
    event_loop.run(move |event, _elwt, control_flow| {
        *control_flow = ControlFlow::Wait;
        if let Event::WindowEvent {
            event: WindowEvent::CloseRequested,
            ..
        } = event
        {
            remote::stop_tunnel(&state);
            if let Ok(st) = state.try_lock() {
                st.settings.save();
            }
            // WebView2のdropがMJPEGストリームの終了を待ってハングすることがあるため、
            // デストラクタを通らず即座にプロセスを終了する
            std::process::exit(0);
        }
    });
}

/// 二重起動防止用のnamed mutex。起動済みならtrue。
#[cfg(windows)]
fn already_running() -> bool {
    use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
    use windows_sys::Win32::System::Threading::CreateMutexW;
    let name: Vec<u16> = "Local\\ShutSingletonMutex\0".encode_utf16().collect();
    unsafe {
        let h = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
        if h.is_null() {
            return false;
        }
        // ハンドルはプロセス終了まで保持し続ける
        GetLastError() == ERROR_ALREADY_EXISTS
    }
}

#[cfg(not(windows))]
fn already_running() -> bool {
    false
}

/// windows_subsystem="windows" では標準出力が無いので、
/// CLIサブコマンド時は親プロセスのコンソールにアタッチして表示できるようにする。
#[cfg(windows)]
fn attach_console() {
    use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

#[cfg(not(windows))]
fn attach_console() {}
