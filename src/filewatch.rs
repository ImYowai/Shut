use std::path::PathBuf;
use std::time::{Duration, SystemTime};

/// セッションログ等のファイル更新を監視する。
/// 監視開始以降に一度でも新しい書き込みを観測し、
/// その後 `idle_minutes` 間更新が止まったら「作業終了」とみなす。
pub struct FileWatcher {
    pub dirs: Vec<PathBuf>,
    /// 監視開始以降の書き込みとして観測した最新のmtime。
    /// 単調増加のみ更新するため、ファイル削除・ローテーションで
    /// 古い時刻へ後退することはない（後退すると放置時間が偽装され誤発火する）。
    last_write: Option<SystemTime>,
    /// 監視開始時刻（これより後の書き込みだけが「活動」）
    arm_time: SystemTime,
    /// 監視開始以降に新規書き込みを観測したか
    pub saw_activity: bool,
}

impl FileWatcher {
    pub fn new() -> Self {
        Self {
            dirs: Vec::new(),
            last_write: None,
            arm_time: SystemTime::now(),
            saw_activity: false,
        }
    }

    /// カンマ/改行区切りのディレクトリ指定をセット（`~` と環境変数を展開）
    pub fn set_dirs(&mut self, raw: &str) {
        self.dirs = raw
            .split([',', '\n'])
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(expand_path)
            .collect();
    }

    /// 監視開始時に呼ぶ。arm時点の既存ファイルは活動として数えない。
    pub fn reset(&mut self) {
        self.arm_time = SystemTime::now();
        self.saw_activity = false;
        self.last_write = None;
        self.refresh();
    }

    /// ディレクトリを再帰走査して、arm以降の書き込みがあれば last_write を更新する。
    /// 厳密に `mtime > arm_time` のものだけを活動とみなす。
    /// 走査時点の既存ファイルや同時刻タイムスタンプは誤検出しない。
    pub fn refresh(&mut self) {
        let mut newest: Option<SystemTime> = None;
        for dir in &self.dirs {
            visit(dir, 0, &mut |mtime| {
                if mtime > self.arm_time && newest.is_none_or(|n| mtime > n) {
                    newest = Some(mtime);
                }
            });
        }
        // 単調にのみ進める: 削除・ローテーションで古い値へ戻さない
        if let Some(m) = newest {
            if self.last_write.is_none_or(|l| m > l) {
                self.last_write = Some(m);
            }
            self.saw_activity = true;
        }
    }

    /// 最新のarm後書き込みからの経過時間
    pub fn since_last_write(&self) -> Option<Duration> {
        self.last_write
            .and_then(|m| SystemTime::now().duration_since(m).ok())
    }
}

fn expand_path(s: &str) -> PathBuf {
    let s = s.trim_matches('"');
    if let Some(rest) = s.strip_prefix('~')
        && let Some(home) = dirs::home_dir()
    {
        return home.join(rest.trim_start_matches(['\\', '/']));
    }
    // %VAR% 形式の環境変数を簡易展開
    let mut out = s.to_string();
    for (k, v) in std::env::vars() {
        let key = format!("%{k}%");
        if out.contains(&key) {
            out = out.replace(&key, &v);
        }
    }
    PathBuf::from(out)
}

fn visit(dir: &PathBuf, depth: usize, f: &mut impl FnMut(SystemTime)) {
    if depth > 8 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in rd.flatten() {
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if meta.is_dir() {
            visit(&entry.path(), depth + 1, f);
        } else if let Ok(mtime) = meta.modified() {
            f(mtime);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tempdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("shut-fw-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn stale_files_do_not_count_as_activity() {
        let dir = tempdir("stale");
        let f = dir.join("old.log");
        fs::write(&f, "old").unwrap();
        // 1時間前に設定
        filetime::set_file_mtime(
            &f,
            filetime::FileTime::from_system_time(SystemTime::now() - Duration::from_secs(3600)),
        )
        .unwrap();

        let mut w = FileWatcher::new();
        w.dirs = vec![dir.clone()];
        w.reset();
        assert!(!w.saw_activity, "既存の古いファイルを活動と誤認");
        assert!(w.since_last_write().is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_after_arm_is_seen_and_ages() {
        let dir = tempdir("post");
        let mut w = FileWatcher::new();
        w.dirs = vec![dir.clone()];
        w.reset();
        assert!(!w.saw_activity);

        std::thread::sleep(Duration::from_millis(20));
        fs::write(dir.join("a.log"), "x").unwrap();
        w.refresh();
        assert!(w.saw_activity);
        assert!(w.since_last_write().unwrap().as_secs() < 5);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn deleted_post_arm_file_does_not_regress_idle() {
        let dir = tempdir("del");
        let stale = dir.join("stale.log");
        fs::write(&stale, "s").unwrap();
        filetime::set_file_mtime(
            &stale,
            filetime::FileTime::from_system_time(SystemTime::now() - Duration::from_secs(7200)),
        )
        .unwrap();

        let mut w = FileWatcher::new();
        w.dirs = vec![dir.clone()];
        w.reset();

        std::thread::sleep(Duration::from_millis(20));
        let b = dir.join("session.log");
        fs::write(&b, "x").unwrap();
        w.refresh();
        assert!(w.saw_activity);
        let ago1 = w.since_last_write().unwrap();

        // 活動ファイルが削除・ローテーション → 残るのは古いファイルのみ
        fs::remove_file(&b).unwrap();
        w.refresh();
        // 古いファイルへ後退して放置時間が巨大化しないこと
        let ago2 = w.since_last_write().unwrap();
        assert!(ago2 >= ago1 && ago2 < Duration::from_secs(60));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_dirs_no_activity() {
        let mut w = FileWatcher::new();
        w.reset();
        w.refresh();
        assert!(!w.saw_activity);
        assert!(w.since_last_write().is_none());
    }
}
