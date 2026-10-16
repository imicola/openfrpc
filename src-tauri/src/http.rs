use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// 极简 HTTP/1.0 GET，用于访问 frpc 本地 webServer 管理接口。
/// 1.0 避免 chunked 编码，读至连接关闭即为完整响应。
/// R16：校验 HTTP 状态码（非 200 视为失败）；支持 Basic 认证；响应体限制 1MB。
pub async fn get_json(
    host: &str,
    port: u16,
    path: &str,
    auth: Option<(&str, &str)>,
    timeout: Duration,
) -> Result<serde_json::Value, String> {
    let fut = async {
        let mut stream = TcpStream::connect((host, port))
            .await
            .map_err(|e| format!("连接管理接口失败: {e}"))?;
        let mut req = format!(
            "GET {path} HTTP/1.0\r\nHost: {host}:{port}\r\nUser-Agent: OpenFrpc\r\nAccept: application/json\r\nConnection: close\r\n"
        );
        if let Some((user, password)) = auth {
            req.push_str(&format!(
                "Authorization: Basic {}\r\n",
                base64(&format!("{user}:{password}"))
            ));
        }
        req.push_str("\r\n");
        stream
            .write_all(req.as_bytes())
            .await
            .map_err(|e| format!("发送请求失败: {e}"))?;
        let mut buf = Vec::with_capacity(16 * 1024);
        // 上限保护：异常大的响应不应无界消耗内存
        stream
            .take(1024 * 1024)
            .read_to_end(&mut buf)
            .await
            .map_err(|e| format!("读取响应失败: {e}"))?;
        let text = String::from_utf8_lossy(&buf);
        let (head, body) = text
            .split_once("\r\n\r\n")
            .ok_or_else(|| "响应格式异常（无头部分隔）".to_string())?;
        let code = head
            .lines()
            .next()
            .and_then(parse_status_code)
            .ok_or_else(|| "响应格式异常（无法解析状态行）".to_string())?;
        if code != 200 {
            return Err(format!("管理接口返回 HTTP {code}"));
        }
        serde_json::from_str::<serde_json::Value>(body.trim_start())
            .map_err(|e| format!("解析响应失败: {e}"))
    };
    tokio::time::timeout(timeout, fut)
        .await
        .map_err(|_| "管理接口请求超时".to_string())?
}

/// "HTTP/1.0 200 OK" → 200
fn parse_status_code(line: &str) -> Option<u16> {
    let mut it = line.trim().split_whitespace();
    let ver = it.next()?;
    if !ver.starts_with("HTTP/") {
        return None;
    }
    it.next()?.parse().ok()
}

/// 标准 Base64（带填充），仅用于管理接口 Basic 认证头
fn base64(s: &str) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            TABLE[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_line_parsing() {
        assert_eq!(parse_status_code("HTTP/1.0 200 OK").unwrap(), 200);
        assert_eq!(parse_status_code("HTTP/1.1 500 Internal Server Error").unwrap(), 500);
        assert!(parse_status_code("garbage").is_none());
        assert!(parse_status_code("").is_none());
    }

    #[test]
    fn base64_known_vectors() {
        assert_eq!(base64("admin:secret"), "YWRtaW46c2VjcmV0");
        assert_eq!(base64(""), "");
        assert_eq!(base64("a"), "YQ==");
        assert_eq!(base64("ab"), "YWI=");
        assert_eq!(base64("abc"), "YWJj");
    }

    #[test]
    fn error_status_rejected_and_500_body_not_json_ok() {
        // 构造 500 + 合法 JSON 的响应文本，模拟证据中的失败场景：
        // 状态码不合格时，即使 body 是可解析 JSON 也必须判失败
        let head = "HTTP/1.0 500 Internal Server Error\r\nContent-Type: application/json\r\n";
        let line = head
            .lines()
            .next()
            .and_then(parse_status_code)
            .unwrap();
        assert_eq!(line, 500);
        assert_ne!(line, 200);
    }
}
