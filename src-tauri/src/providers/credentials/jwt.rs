//! JWT payload 解码。
//!
//! **只解 payload，不验证签名。** 用途仅限读取本地凭据里的 `exp` 与身份 claim；
//! 授权判断由 Provider 服务端完成，CC Trace 不做任何基于该 payload 的信任决策，
//! 见 `docs/额度领域模型.md` 第 5.2 节。

use chrono::{DateTime, Utc};

/// 解出 JWT 的 payload 段。输入不是 JWT、段数不对或 base64url 无效时返回 `None`。
pub fn decode_payload(token: &str) -> Option<serde_json::Value> {
    let mut segments = token.split('.');
    let _header = segments.next()?;
    let payload = segments.next()?;
    segments.next()?;

    let bytes = decode_base64url(payload)?;
    serde_json::from_slice(&bytes).ok()
}

/// 读取 payload 的 `exp`，转成 UTC 时刻。
pub fn expires_at(token: &str) -> Option<DateTime<Utc>> {
    let payload = decode_payload(token)?;
    let exp = payload.get("exp")?.as_f64()?;
    if !exp.is_finite() {
        return None;
    }

    DateTime::from_timestamp(exp as i64, 0)
}

/// base64url（RFC 4648 §5）解码，padding 可有可无。
fn decode_base64url(input: &str) -> Option<Vec<u8>> {
    let input = input.trim_end_matches('=');

    let mut output = Vec::with_capacity(input.len() * 3 / 4);
    let mut buffer: u32 = 0;
    let mut bits: u32 = 0;

    for byte in input.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'-' => 62,
            b'_' => 63,
            _ => return None,
        } as u32;

        buffer = (buffer << 6) | value;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            output.push((buffer >> bits) as u8);
            buffer &= (1 << bits) - 1;
        }
    }

    // 剩余不足一个字节的位必须是填充零，否则输入被截断过。
    if buffer != 0 {
        return None;
    }

    Some(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 结构真实但内容全部虚构的 token：header 与 payload 是 base64url，签名段是占位符。
    /// payload 为 `{"exp":1700000000,"email":"user@example.test","plan":"pro"}`。
    const FAKE_TOKEN: &str = concat!(
        "eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCJ9",
        ".",
        "eyJleHAiOjE3MDAwMDAwMDAsImVtYWlsIjoidXNlckBleGFtcGxlLnRlc3QiLCJwbGFuIjoicHJvIn0",
        ".",
        "c2lnbmF0dXJlLXBsYWNlaG9sZGVy"
    );

    #[test]
    fn decodes_the_payload_of_a_well_formed_token() {
        let payload = decode_payload(FAKE_TOKEN).expect("payload decodes");
        assert_eq!(payload["email"], "user@example.test");
        assert_eq!(payload["plan"], "pro");
    }

    #[test]
    fn reads_the_expiry_as_a_utc_instant() {
        assert_eq!(
            expires_at(FAKE_TOKEN),
            DateTime::from_timestamp(1_700_000_000, 0)
        );
    }

    #[test]
    fn malformed_tokens_decode_to_nothing_instead_of_panicking() {
        for input in [
            "",
            "not-a-jwt",
            "only.two",
            "bad!!.bad!!.bad!!",
            "eyJhbGciOiJSUzI1NiJ9.bm90LWpzb24.sig",
        ] {
            assert_eq!(decode_payload(input), None, "{input:?} must not decode");
            assert_eq!(
                expires_at(input),
                None,
                "{input:?} must not yield an expiry"
            );
        }
    }

    #[test]
    fn an_opaque_token_without_an_exp_has_no_expiry() {
        // payload 为 `{"sub":"abc"}`：合法 JWT，但没有 exp。
        let token = "eyJhbGciOiJSUzI1NiJ9.eyJzdWIiOiJhYmMifQ.sig";
        assert!(decode_payload(token).is_some());
        assert_eq!(expires_at(token), None);
    }
}
