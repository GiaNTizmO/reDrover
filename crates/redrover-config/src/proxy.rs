//! Proxy URL parsing.
//!
//! Mirrors the regex and rules from the original Pascal `TProxyValue.ParseFromString`:
//!   [proto://][user:pass@]host:port
//! - default protocol: `http`
//! - `https` is normalized to `http` (no upstream TLS to the proxy itself)
//! - `socks5` is the only other accepted protocol
//! - SOCKS5 + auth is rejected by the GUI but accepted by the parser
//!   (validation happens in the GUI layer).

use regex::Regex;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProxyKind {
    #[default]
    Http,
    Socks5,
}

impl ProxyKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::Socks5 => "socks5",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProxyValue {
    pub is_specified: bool,
    pub kind: ProxyKind,
    pub login: String,
    pub password: String,
    pub host: String,
    pub port: u16,
}

impl ProxyValue {
    pub fn parse(url: &str) -> Self {
        static RE: OnceLock<Regex> = OnceLock::new();
        let re = RE.get_or_init(|| {
            Regex::new(r"(?i)\A(?:([a-z\d]+)://)?(?:(.+):(.+)@)?(.+):(\d+)\z").unwrap()
        });

        let trimmed = url.trim();
        let Some(caps) = re.captures(trimmed) else {
            return Self::default();
        };

        let proto_raw = caps.get(1).map(|m| m.as_str()).unwrap_or("").to_ascii_lowercase();
        let kind = match proto_raw.as_str() {
            "socks5" => ProxyKind::Socks5,
            _ => ProxyKind::Http,
        };

        let login = caps.get(2).map(|m| m.as_str().trim().to_string()).unwrap_or_default();
        let password = caps.get(3).map(|m| m.as_str().trim().to_string()).unwrap_or_default();
        let host = caps.get(4).map(|m| m.as_str().trim().to_string()).unwrap_or_default();
        let port: u16 = caps.get(5).and_then(|m| m.as_str().parse().ok()).unwrap_or(0);

        Self {
            is_specified: true,
            kind,
            login,
            password,
            host,
            port,
        }
    }

    pub fn has_auth(&self) -> bool {
        !self.login.is_empty() && !self.password.is_empty()
    }

    /// Format for `http_proxy` / `https_proxy` environment variables.
    pub fn format_http_env(&self) -> String {
        if !self.is_specified {
            return String::new();
        }
        let mut s = String::from("http://");
        if self.has_auth() {
            s.push_str(&format!("{}:{}@", self.login, self.password));
        }
        s.push_str(&format!("{}:{}", self.host, self.port));
        s
    }

    /// Format for Chromium `--proxy-server=` flag.
    pub fn format_chrome_proxy(&self) -> String {
        if !self.is_specified {
            return String::new();
        }
        format!("{}://{}:{}", self.kind.as_str(), self.host, self.port)
    }

    /// Format back as a canonical URL for `drover.ini`.
    pub fn format_url(&self) -> String {
        if !self.is_specified {
            return String::new();
        }
        let mut s = format!("{}://", self.kind.as_str());
        if self.has_auth() {
            s.push_str(&format!("{}:{}@", self.login, self.password));
        }
        s.push_str(&format!("{}:{}", self.host, self.port));
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_http() {
        let v = ProxyValue::parse("http://1.2.3.4:8080");
        assert!(v.is_specified);
        assert_eq!(v.kind, ProxyKind::Http);
        assert_eq!(v.host, "1.2.3.4");
        assert_eq!(v.port, 8080);
        assert!(!v.has_auth());
    }

    #[test]
    fn parses_socks_with_auth() {
        let v = ProxyValue::parse("socks5://u:p@example.com:1080");
        assert_eq!(v.kind, ProxyKind::Socks5);
        assert!(v.has_auth());
    }

    #[test]
    fn https_normalizes_to_http() {
        let v = ProxyValue::parse("https://host:443");
        assert_eq!(v.kind, ProxyKind::Http);
    }

    #[test]
    fn empty_is_not_specified() {
        let v = ProxyValue::parse("");
        assert!(!v.is_specified);
    }
}
