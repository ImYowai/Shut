//! ホストモードの起動。HTTPサーバー + トリガー評価ループを立ち上げる。
//! Dioxus ホスト画面から1回だけ呼ばれる（OnceLock で冪等）。

use std::net::{SocketAddr, TcpListener};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use crate::filewatch::FileWatcher;
use crate::server::ServerCtx;
use crate::state::{AppState, FwStatus, Phase};
use crate::{log, power, remote, server};

#[derive(Clone)]
pub struct HostInfo {
    pub port: u16,
    pub lan_url: String,
}

static STARTED: OnceLock<Result<HostInfo, String>> = OnceLock::new();

pub fn start(state: &AppState) -> Result<HostInfo, String> {
    STARTED.get_or_init(|| start_inner(state)).clone()
}

fn start_inner(state: &AppState) -> Result<HostInfo, String> {
    let (listener, port) = bind_listener();
    listener
        .set_nonblocking(true)
        .map_err(|e| e.to_string())?;
    let lan_url = remote::lan_ip()
        .map(|ip| format!("http://{ip}:{port}"))
        .unwrap_or_else(|| format!("http://127.0.0.1:{port}"));

    let ctx = Arc::new(ServerCtx {
        state: state.clone(),
        port,
        lan_url: lan_url.clone(),
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
            let app = server::router(ctx)
                .into_make_service_with_connect_info::<SocketAddr>();
            let _ = axum::serve(tl, app).await;
        });
    });

    log::line(&format!("host: listening on {lan_url}"));
    Ok(HostInfo { port, lan_url })
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

/// 1秒間隔でトリガー評価とカウントダウン進行を行う。
fn eval_loop(state: AppState) {
    let mut fw = FileWatcher::new();
    let mut cur_dirs = String::new();
    let mut was_armed = false;
    let mut last_fw = Instant::now() - Duration::from_secs(3);
    loop {
        std::thread::sleep(Duration::from_secs(1));

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
                        };
                    }
                    Err(e) => {
                        log::line(&format!("shutdown failed: {e}"));
                        st.phase = Phase::Done {
                            message: format!("シャットダウンに失敗しました: {e}"),
                        };
                    }
                }
            }
            _ => {}
        }
    }
}
