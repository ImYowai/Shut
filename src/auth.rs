//! パスワード認証 (argon2) と認証試行のレート制限。
//!
//! - パスワードは argon2id ハッシュとして config.json に永続化（平文は置かない）
//! - /api/auth で検証成功するとセッショントークン(32B hex)を発行
//! - 同一IPからの連続失敗で指数的に待たせる（60sでリセット）

use std::collections::HashMap;
use std::net::IpAddr;
use std::time::{Duration, Instant};

use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;

/// セッショントークンの有効期間（発行から24時間）
const TOKEN_TTL: Duration = Duration::from_secs(24 * 3600);
/// 同時有効トークンの上限。超えたら最古のものを失効させる。
const MAX_TOKENS: usize = 64;

#[derive(Default)]
pub struct AuthState {
    /// 発行済みセッショントークン -> 発行時刻
    tokens: HashMap<String, Instant>,
    /// IP -> (連続失敗回数, 次に試行可能な時刻)
    failures: HashMap<IpAddr, (u32, Instant)>,
}

impl AuthState {
    pub fn valid_token(&self, t: &str) -> bool {
        self.tokens.get(t).is_some_and(|i| i.elapsed() < TOKEN_TTL)
    }

    /// 認証試行。Ok(token) か Err(待ち秒 or 理由)。
    pub fn attempt(&mut self, ip: IpAddr, password: &str, hash: Option<&str>) -> Result<String, String> {
        let now = Instant::now();
        if let Some((_, next_ok)) = self.failures.get(&ip)
            && now < *next_ok
        {
            let left = (*next_ok - now).as_secs() + 1;
            return Err(format!("しばらく待ってください（{left}秒後に再試行可）"));
        }

        let ok = match hash {
            Some(h) => verify(h, password),
            None => return Err("ホスト側でパスワードが未設定です".into()),
        };
        if !ok {
            let (n, _) = self.failures.get(&ip).copied().unwrap_or((0, now));
            let n = n + 1;
            // 3回失敗ごとに 2^n 秒待たせる（最大 ~4分）
            let backoff = if n >= 3 {
                Duration::from_secs((1u64 << (n - 2).min(8)) * 2)
            } else {
                Duration::ZERO
            };
            self.failures.insert(ip, (n, now + backoff));
            return Err("パスワードが違います".into());
        }

        self.failures.remove(&ip);
        // 期限切れを掃除し、上限を超えるなら最古を失効させる
        self.tokens.retain(|_, i| i.elapsed() < TOKEN_TTL);
        if self.tokens.len() >= MAX_TOKENS
            && let Some(oldest) = self
                .tokens
                .iter()
                .max_by_key(|(_, i)| i.elapsed())
                .map(|(t, _)| t.clone())
        {
            self.tokens.remove(&oldest);
        }
        let token = format!("{:032x}", rand::random::<u128>());
        self.tokens.insert(token.clone(), now);
        Ok(token)
    }
}

/// パスワード -> argon2id ハッシュ文字列 (PHC形式)
pub fn hash_password(pw: &str) -> Result<String, String> {
    let salt_bytes: [u8; 16] = rand::random();
    let salt = SaltString::encode_b64(&salt_bytes).map_err(|e| e.to_string())?;
    Argon2::default()
        .hash_password(pw.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| format!("ハッシュ生成に失敗: {e}"))
}

fn verify(hash: &str, pw: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(pw.as_bytes(), &parsed)
        .is_ok()
}
