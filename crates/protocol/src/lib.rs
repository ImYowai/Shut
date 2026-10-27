//! Shut のホスト・クライアント間で共有するプロトコル型。
//! JSON で直列化して REST/WebSocket/DataChannel のいずれでも運べる形にする。

use serde::{Deserialize, Serialize};

/// クライアント→ホストの入力イベント。
/// `kind` で内容が決まる:
/// `down`/`up`/`move`/`click`/`double_click`/`scroll` … x,y (0.0-1.0) + button/dy
/// `key` … key 名 + modifiers
/// `text` … text (Unicode注入)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputEvent {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub x: f64,
    #[serde(default)]
    pub y: f64,
    #[serde(default)]
    pub button: String,
    #[serde(default)]
    pub dy: f64,
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub modifiers: Vec<String>,
    #[serde(default)]
    pub text: String,
    /// 対象モニタ（ミラー表示と同じインデックス）
    #[serde(default)]
    pub m: usize,
}

impl InputEvent {
    pub fn pointer(kind: &str, x: f64, y: f64) -> Self {
        Self {
            kind: kind.into(),
            x,
            y,
            ..Default::default()
        }
    }

    pub fn key(name: &str, modifiers: Vec<String>) -> Self {
        Self {
            kind: "key".into(),
            key: name.into(),
            modifiers,
            ..Default::default()
        }
    }

    pub fn text(text: impl Into<String>) -> Self {
        Self {
            kind: "text".into(),
            text: text.into(),
            ..Default::default()
        }
    }
}

impl Default for InputEvent {
    fn default() -> Self {
        Self {
            kind: String::new(),
            x: 0.0,
            y: 0.0,
            button: String::new(),
            dy: 0.0,
            key: String::new(),
            modifiers: Vec::new(),
            text: String::new(),
            m: 0,
        }
    }
}

// ---------- 接続URLの検証（全クライアント共通） ----------

mod connection {
    use std::net::IpAddr;
    use url::{Host, Url};

    /// No Debug/Display: the URL contains a bearer token and must not enter logs.
    #[derive(Clone, PartialEq)]
    pub struct ConnectionTarget {
        url: Url,
        pub endpoint: String,
        pub cleartext: bool,
    }

    impl ConnectionTarget {
        /// Contract v2: remote HTML disables persistent token storage in this mode.
        pub fn ephemeral_url(&self) -> String {
            let mut url = self.url.clone();
            if !url.query_pairs().any(|(key, _)| key == "ephemeral") {
                url.query_pairs_mut().append_pair("ephemeral", "1");
            }
            url.to_string()
        }

        /// ネイティブクライアント用: クエリを除いた `scheme://authority`。
        pub fn base_url(&self) -> String {
            format!("{}/", self.url.origin().ascii_serialization())
        }

        /// ネイティブクライアント用: 接続トークン。
        pub fn token(&self) -> String {
            self.url
                .query_pairs()
                .find(|(key, _)| key == "t")
                .map(|(_, value)| value.into_owned())
                .unwrap_or_default()
        }
    }

    pub fn validate_connection(input: &str) -> Result<ConnectionTarget, &'static str> {
        let value = input.trim();
        if value.is_empty() {
            return Err("PCに表示された接続URLを入力してください。");
        }
        if value.len() > 2048
            || value.chars().any(|c| c.is_control() || c.is_whitespace())
            || value.contains('\\')
        {
            return Err("接続URLの形式を確認してください。");
        }
        let (_, remainder) = value
            .split_once("://")
            .ok_or("http:// または https:// で始まるURLが必要です。")?;
        let authority_end = remainder.find(['/', '?', '#']).unwrap_or(remainder.len());
        if remainder[..authority_end].contains('@') {
            return Err("ユーザー名やパスワードを含むURLには接続できません。");
        }
        let tail = &remainder[authority_end..];
        let raw_path = tail.split(['?', '#']).next().unwrap_or_default();
        if !raw_path.is_empty() && raw_path != "/" {
            return Err("Shutのトップページの接続URLを使用してください。");
        }
        let parsed = Url::parse(value).map_err(|_| "接続URLを読み取れませんでした。")?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err("http:// または https:// のURLだけに接続できます。");
        }
        if !parsed.username().is_empty() || parsed.password().is_some() {
            return Err("ユーザー名やパスワードを含むURLには接続できません。");
        }
        if parsed.fragment().is_some() || parsed.path() != "/" || parsed.port() == Some(0) {
            return Err("Shutのトップページの接続URLを使用してください。");
        }
        let mut token = None;
        let mut ephemeral = false;
        for (key, value) in parsed.query_pairs() {
            match key.as_ref() {
                "t" if token.is_none() => token = Some(value.into_owned()),
                "ephemeral" if !ephemeral && value == "1" => ephemeral = true,
                _ => {
                    return Err(
                        "接続URLに未対応の項目があります。PCからURLをコピーし直してください。",
                    );
                }
            }
        }
        // トークンは任意（無い場合はパスワード認証で後からセッション取得）。
        // ある場合は形式を厳密に検証する。
        if let Some(value) = token
            && !(value.len() == 32 && value.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            return Err("32桁の接続トークン（?t=...）の形式が不正です。");
        }
        let host = parsed.host().ok_or("接続先のアドレスが必要です。")?;
        let cleartext = parsed.scheme() == "http";
        if cleartext {
            let local = match host {
                Host::Ipv4(v4) => is_local(IpAddr::V4(v4)),
                Host::Ipv6(v6) => is_local(IpAddr::V6(v6)),
                Host::Domain(name) => name.eq_ignore_ascii_case("localhost"),
            };
            if !local {
                return Err(
                    "HTTP接続は同じネットワークのIPアドレスに限ります。外部接続はHTTPSを使用してください。",
                );
            }
        }
        Ok(ConnectionTarget {
            endpoint: parsed.origin().ascii_serialization(),
            url: parsed,
            cleartext,
        })
    }

    fn is_local(ip: IpAddr) -> bool {
        match ip {
            IpAddr::V4(v4) => v4.is_private() || v4.is_loopback() || v4.is_link_local(),
            IpAddr::V6(v6) => {
                v6.is_loopback() || v6.is_unique_local() || v6.is_unicast_link_local()
            }
        }
    }
}

pub use connection::{ConnectionTarget, validate_connection};

#[cfg(test)]
mod tests {
    use super::*;
    const TOKEN: &str = "0123456789abcdef0123456789abcdef";
    #[test]
    fn accepts_lan_and_https_without_exposing_token_in_endpoint() {
        for base in [
            "http://192.168.1.20:47823/",
            "http://10.0.0.10:47823/",
            "http://172.16.0.1:47823/",
            "http://169.254.10.2:47823/",
            "http://127.0.0.1:47823/",
            "http://localhost:47823/",
            "http://[::1]:47823/",
            "http://[fd00::1]:47823/",
            "http://[fe80::1]:47823/",
            "https://any-custom.example/",
            "https://shut-demo.trycloudflare.com/",
        ] {
            let target = validate_connection(&format!("{base}?t={TOKEN}"));
            assert!(target.is_ok(), "expected supported base: {base}");
            let target = target.unwrap();
            assert!(!target.endpoint.contains(TOKEN));
            assert_eq!(target.cleartext, base.starts_with("http:"));
            assert_eq!(target.token(), TOKEN);
        }
    }
    #[test]
    fn ephemeral_contract_is_appended_once() {
        for query in [format!("t={TOKEN}"), format!("t={TOKEN}&ephemeral=1")] {
            let target = validate_connection(&format!("https://example.com/?{query}")).unwrap();
            let url = url::Url::parse(&target.ephemeral_url()).unwrap();
            let pairs: Vec<_> = url.query_pairs().collect();
            assert_eq!(
                pairs
                    .iter()
                    .filter(|(key, value)| key == "ephemeral" && value == "1")
                    .count(),
                1
            );
            assert_eq!(pairs.len(), 2);
        }
    }
    #[test]
    fn refuses_external_cleartext_and_non_web_schemes() {
        for base in [
            "http://8.8.8.8/",
            "http://example.com/",
            "http://192.168.1.2.example.com/",
            "http://[2001:4860:4860::8888]/",
            "file:///",
            "content://media/",
            "intent://example.com/",
            "javascript://example.com/",
        ] {
            assert!(validate_connection(&format!("{base}?t={TOKEN}")).is_err());
        }
    }
    #[test]
    fn refuses_ambiguous_or_injected_urls() {
        for value in [
            format!("https://user:pass@example.com/?t={TOKEN}"),
            format!("https://@example.com/?t={TOKEN}"),
            format!("https://example.com/not-shut?t={TOKEN}"),
            format!("https://example.com/../?t={TOKEN}"),
            format!("https://example.com/%2e/?t={TOKEN}"),
            format!("https://example.com/?t={TOKEN}#"),
            format!("https://example.com/?t={TOKEN}&t={TOKEN}"),
            format!("https://example.com/?t={TOKEN}&next=https://evil.example"),
            format!("https://example.com/?t={TOKEN}&ephemeral=0"),
            format!("https://example.com/?t={TOKEN}&ephemeral=1&ephemeral=1"),
            format!("https://example.com:0/?t={TOKEN}"),
            format!("https://example.com\\evil?t={TOKEN}"),
            format!("https://exam\nple.com/?t={TOKEN}"),
            "https://example.com/?t=not-a-token".into(),
            "https://example.com/?t=0123456789abcdef0123456789abcdeg".into(),
        ] {
            assert!(validate_connection(&value).is_err());
        }
    }
}
