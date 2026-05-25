//! UDP-strategy configuration.
//!
//! ⭐ This module is intentionally over-modeled. It exists to give a clean,
//! typed surface for implementing the ideas in `ISSUE-65-IDEAS.md`:
//!
//! - §2.1 — configurable prefix packets and delay (`prefix_packets`, `prefix_delay_ms`)
//! - §2.2 — named strategies (`UdpStrategy::*`)
//! - §2.3 — splitting the first packet (`split_first`)
//! - §2.4 — payload sets shipped as `dist/strategies/*.bin`
//! - §2.6 — force-TCP-fallback (`force_tcp_fallback`)
//!
//! The C++ DLL reads the same fields from `drover.ini`; keep them in sync.

use ini::Properties;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UdpStrategy {
    /// Don't touch outgoing UDP at all.
    Off,
    /// Original Pascal behavior: prefix bytes 0x00, 0x01 + Sleep(50) + original packet.
    Classic,
    /// UAE-focused tuning (placeholder name — actual bytes live in dist/strategies/).
    UaeV1,
    /// UAE-focused tuning, variant 2.
    UaeV2,
    /// Split the first 74-byte IP discovery packet into N parts.
    Split,
    /// Use whatever is specified by `prefix_packets`, `split_first`, etc.
    Custom,
}

impl Default for UdpStrategy {
    fn default() -> Self {
        Self::Classic
    }
}

impl UdpStrategy {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Classic => "classic",
            Self::UaeV1 => "uae_v1",
            Self::UaeV2 => "uae_v2",
            Self::Split => "split",
            Self::Custom => "custom",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "off" | "disable" | "disabled" => Self::Off,
            "classic" | "" => Self::Classic,
            "uae_v1" | "uae" => Self::UaeV1,
            "uae_v2" => Self::UaeV2,
            "split" => Self::Split,
            "custom" => Self::Custom,
            _ => Self::Classic,
        }
    }

    pub const ALL: &'static [Self] = &[
        Self::Off,
        Self::Classic,
        Self::UaeV1,
        Self::UaeV2,
        Self::Split,
        Self::Custom,
    ];
}

impl std::fmt::Display for UdpStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UdpSettings {
    pub strategy: UdpStrategy,
    pub prefix_delay_ms: u32,
    /// Each entry is a separate UDP packet sent before the original one.
    pub prefix_packets: Vec<Vec<u8>>,
    /// 0 = don't split. N>1 = split the first original packet into N parts.
    pub split_first: u8,
    pub force_tcp_fallback: bool,
}

impl Default for UdpSettings {
    fn default() -> Self {
        Self {
            strategy: UdpStrategy::Classic,
            prefix_delay_ms: 50,
            prefix_packets: vec![vec![0x00], vec![0x01]],
            split_first: 0,
            force_tcp_fallback: false,
        }
    }
}

impl UdpSettings {
    pub fn from_section(section: Option<&Properties>) -> Self {
        let mut out = Self::default();
        let Some(s) = section else { return out };

        if let Some(v) = s.get("strategy") {
            out.strategy = UdpStrategy::parse(v);
        }
        if let Some(v) = s.get("prefix_delay_ms").and_then(|s| s.parse().ok()) {
            out.prefix_delay_ms = v;
        }
        if let Some(v) = s.get("prefix_packets") {
            out.prefix_packets = parse_prefix_packets(v);
        }
        if let Some(v) = s.get("split_first").and_then(|s| s.parse().ok()) {
            out.split_first = v;
        }
        if let Some(v) = s.get("force_tcp_fallback") {
            out.force_tcp_fallback = matches!(
                v.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            );
        }
        out
    }
}

/// Parse `"00, 01, deadbeef"` → `[[0x00], [0x01], [0xde, 0xad, 0xbe, 0xef]]`.
pub fn parse_prefix_packets(input: &str) -> Vec<Vec<u8>> {
    input
        .split(|c: char| c == ',' || c == ';' || c.is_whitespace())
        .filter(|s| !s.is_empty())
        .filter_map(|hex| {
            let hex = hex.trim().trim_start_matches("0x").trim_start_matches("0X");
            if hex.is_empty() || hex.len() % 2 != 0 {
                return None;
            }
            let mut bytes = Vec::with_capacity(hex.len() / 2);
            for chunk in hex.as_bytes().chunks(2) {
                let s = std::str::from_utf8(chunk).ok()?;
                bytes.push(u8::from_str_radix(s, 16).ok()?);
            }
            Some(bytes)
        })
        .collect()
}

pub fn format_prefix_packets(packets: &[Vec<u8>]) -> String {
    packets
        .iter()
        .map(|p| p.iter().map(|b| format!("{:02x}", b)).collect::<String>())
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strategy_roundtrip() {
        for s in UdpStrategy::ALL {
            assert_eq!(UdpStrategy::parse(s.as_str()), *s);
        }
    }

    #[test]
    fn parses_prefix_packets() {
        assert_eq!(
            parse_prefix_packets("00, 01, deadbeef"),
            vec![vec![0x00], vec![0x01], vec![0xde, 0xad, 0xbe, 0xef]]
        );
    }

    #[test]
    fn formats_prefix_packets() {
        let v = vec![vec![0x00], vec![0xab, 0xcd]];
        assert_eq!(format_prefix_packets(&v), "00, abcd");
    }
}
