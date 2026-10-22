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
                return Err("接続URLに未対応の項目があります。PCからURLをコピーし直してください。");
            }
        }
    }
    if !token.is_some_and(|value| {
        value.len() == 32 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
    }) {
        return Err("32桁の接続トークン（?t=...）を含むURLが必要です。");
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
        IpAddr::V6(v6) => v6.is_loopback() || v6.is_unique_local() || v6.is_unicast_link_local(),
    }
}

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
        }
    }
    #[test]
    fn ephemeral_contract_is_appended_once() {
        for query in [format!("t={TOKEN}"), format!("t={TOKEN}&ephemeral=1")] {
            let target = validate_connection(&format!("https://example.com/?{query}")).unwrap();
            let url = Url::parse(&target.ephemeral_url()).unwrap();
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
            "https://example.com/".into(),
            "https://example.com/?t=not-a-token".into(),
            "https://example.com/?t=0123456789abcdef0123456789abcdeg".into(),
        ] {
            assert!(validate_connection(&value).is_err());
        }
    }
}
