use std::path::PathBuf;
use std::time::{Duration, SystemTime};

/// セッションログ等のファイル更新を監視する。
/// 監視開始以降に一度でも新しい書き込みを観測し、
/// その後 `idle_minutes` 間更新が止まったら「作業終了」とみなす。
pub struct FileWatcher {
    pub dirs: Vec<PathBuf>,
    /// 観測した最新のmtime
    pub newest_mtime: Option<SystemTime>,
    /// 監視開始時刻（これ以降の書き込みがあれば「活動あり」とする）
    arm_time: SystemTime,
    /// 監視開始以降に新規書き込みを観測したか
    pub saw_activity: bool,
}

impl FileWatcher {
    pub fn new() -> Self {
        Self {
            dirs: Vec::new(),
            newest_mtime: None,
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

    /// 監視開始時に呼ぶ
    pub fn reset(&mut self) {
        self.arm_time = SystemTime::now();
        self.saw_activity = false;
        self.refresh();
    }

    /// ディレクトリを再帰走査して最新mtimeを更新する
    pub fn refresh(&mut self) {
        let mut newest: Option<SystemTime> = None;
        for dir in &self.dirs {
            visit(dir, 0, &mut |mtime| {
                if newest.is_none_or(|n| mtime > n) {
                    newest = Some(mtime);
                }
            });
        }
        self.newest_mtime = newest;
        if newest.is_some_and(|m| m >= self.arm_time) {
            self.saw_activity = true;
        }
    }

    /// 最新更新からの経過時間
    pub fn since_last_write(&self) -> Option<Duration> {
        self.newest_mtime
            .and_then(|m| SystemTime::now().duration_since(m).ok())
    }

}

fn expand_path(s: &str) -> PathBuf {
    let s = s.trim_matches('"');
    if let Some(rest) = s.strip_prefix('~') {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest.trim_start_matches(['\\', '/']));
        }
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
