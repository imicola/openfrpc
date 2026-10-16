use serde::Serialize;

/// 一条解析后的 frpc 日志
#[derive(Serialize, Clone, Debug)]
pub struct LogLine {
    pub id: u64,
    /// 原始时间戳 "2026-10-16 15:18:31.767"，为空时用本机时间
    pub ts: String,
    pub level: String,
    pub src: String,
    pub msg: String,
}

pub struct ParsedLine {
    pub ts: String,
    pub level: String,
    pub src: String,
    pub run_id: Option<String>,
    pub msg: String,
}

/// 去除 ANSI 转义序列与 \r
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if chars.peek() == Some(&'[') {
                chars.next();
                // 消费到终结字母为止
                while let Some(&c2) = chars.peek() {
                    chars.next();
                    if c2.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
        } else if c != '\r' {
            out.push(c);
        }
    }
    out
}

/// 解析 frpc 标准日志行：
/// `2026-10-16 15:18:31.767 [I] [client/service.go:335] [670f961effdf576c] login to server success, get run id [...]`
pub fn parse_frpc_line(raw: &str) -> ParsedLine {
    let line = strip_ansi(raw);
    let trimmed = line.trim_start();
    let bytes = trimmed.as_bytes();

    // 时间戳固定 23 字符
    let (ts, rest) = if bytes.len() > 24
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[10] == b' '
        && bytes[13] == b':'
    {
        (trimmed[..23].to_string(), &trimmed[24..])
    } else {
        (String::new(), trimmed)
    };

    // 级别 [I]/[W]/[E]/[D]
    let (level, rest) = if rest.starts_with('[') {
        match rest.find(']') {
            Some(i) => (rest[1..i].trim().to_string(), &rest[i + 1..]),
            None => ("I".to_string(), rest),
        }
    } else {
        ("I".to_string(), rest)
    };

    // 来源 [client/service.go:335]
    let rest_t = rest.trim_start();
    let (src, rest) = if rest_t.starts_with('[') {
        match rest_t.find(']') {
            Some(i) => (rest_t[1..i].to_string(), &rest_t[i + 1..]),
            None => (String::new(), rest_t),
        }
    } else {
        (String::new(), rest_t)
    };

    // 运行 ID [670f961effdf576c]，16 个十六进制字符
    let rest_t = rest.trim_start();
    let (run_id, rest) = if rest_t.starts_with('[') && rest_t.find(']').map(|i| i == 17).unwrap_or(false) {
        (Some(rest_t[1..17].to_string()), &rest_t[18..])
    } else {
        (None, rest_t)
    };

    ParsedLine {
        ts,
        level,
        src,
        run_id,
        msg: rest.trim().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_standard_line() {
        let p = parse_frpc_line("\u{1b}[0m\u{1b}[1;34m2026-10-16 15:18:34.497 [I] [client/service.go:355] [670f961effdf576c] login to server success, get run id [670f961effdf576c]\r");
        assert_eq!(p.ts, "2026-10-16 15:18:34.497");
        assert_eq!(p.level, "I");
        assert_eq!(p.src, "client/service.go:355");
        assert_eq!(p.run_id.as_deref(), Some("670f961effdf576c"));
        assert!(p.msg.starts_with("login to server success"));
    }

    #[test]
    fn parses_line_without_runid() {
        let p = parse_frpc_line("2026-10-16 15:18:34.703 [I] [proxy/proxy_manager.go:183] [670f961effdf576c] proxy added: [demo]");
        assert_eq!(p.level, "I");
        assert_eq!(p.msg, "proxy added: [demo]");
    }

    #[test]
    fn falls_back_gracefully() {
        let p = parse_frpc_line("some free-form text");
        assert_eq!(p.ts, "");
        assert_eq!(p.msg, "some free-form text");
    }
}
