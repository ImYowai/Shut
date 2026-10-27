use std::io::Write;
use std::path::PathBuf;

fn log_file() -> PathBuf {
    crate::data_dir().join("shut.log")
}

/// 監視・シャットダウン実行の証跡を残す。失敗しても本体を止めない。
pub fn line(msg: &str) {
    let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_file())
    {
        let _ = writeln!(f, "[{ts}] {msg}");
    }
}
