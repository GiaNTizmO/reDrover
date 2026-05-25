//! Shared configuration types for Redrover.
//!
//! This crate is the single source of truth for the on-disk `drover.ini`
//! format. The GUI installer and any future helpers (CLI, log viewer)
//! depend on it. The C++ DLL keeps its own parallel parser in
//! `dll/src/config.cpp`; if the format changes, update both. See
//! `docs/ARCHITECTURE.md` for why.
//!
//! ## Format
//!
//! ```ini
//! [drover]
//! proxy = http://user:pass@host:port    ; empty => Direct mode
//!
//! [udp]
//! ; ⭐ Knobs surfaced specifically for ISSUE-65-IDEAS.md:
//! strategy = uae_v1     ; off | classic | uae_v1 | uae_v2 | split | custom
//! prefix_delay_ms = 50
//! prefix_packets = 00, 01           ; hex bytes; each entry is one UDP packet
//! split_first = 0                   ; 0 = don't split; N>1 = split into N parts
//! force_tcp_fallback = false        ; intentionally break first UDP packet
//!
//! [socks5]
//! ; ⭐ For SOCKS5 UDP ASSOCIATE (ISSUE-65-IDEAS.md §3.2)
//! udp_associate = false
//!
//! [logging]
//! level = info                      ; off | error | warn | info | debug | trace
//! file_enabled = true               ; write log messages to `file`
//! file = drover.log                 ; relative to Discord.exe folder
//! ```

pub mod proxy;
pub mod udp;

use std::path::Path;

use anyhow::{Context, Result};
use ini::Ini;

pub use proxy::{ProxyKind, ProxyValue};
pub use udp::{UdpSettings, UdpStrategy};

pub const DLL_FILENAME: &str = "version.dll";
pub const OPTIONS_FILENAME: &str = "drover.ini";
pub const PACKET_FILENAME: &str = "drover-packet.bin";
pub const LOG_FILENAME_DEFAULT: &str = "drover.log";

/// Top-level config, parsed from `drover.ini`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DroverOptions {
    pub proxy: String,
    pub udp: UdpSettings,
    pub socks5_udp_associate: bool,
    pub log_level: Option<String>,
    /// Write log output to `log_file`. Missing keys preserve the historical
    /// behavior of writing `drover.log`.
    pub log_file_enabled: bool,
    pub log_file: Option<String>,
    /// On Windows: spawn a console window via `AllocConsole` and mirror
    /// log output there. On POSIX: still routes log output to stderr.
    /// See ISSUE-65-IDEAS.md §4.3 — diagnostic logging.
    pub log_console: bool,
}

impl Default for DroverOptions {
    fn default() -> Self {
        Self {
            proxy: String::new(),
            udp: UdpSettings::default(),
            socks5_udp_associate: false,
            log_level: Some("info".to_string()),
            log_file_enabled: true,
            log_file: Some(LOG_FILENAME_DEFAULT.to_string()),
            log_console: false,
        }
    }
}

impl DroverOptions {
    /// Parse from disk. A missing file yields `Default::default()` rather
    /// than an error — matches the Pascal `LoadOptions` behavior.
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let ini = Ini::load_from_file(path)
            .with_context(|| format!("failed to parse {}", path.display()))?;
        Ok(Self::from_ini(&ini))
    }

    pub fn from_ini(ini: &Ini) -> Self {
        let drover = ini.section(Some("drover"));
        let udp = ini.section(Some("udp"));
        let socks5 = ini.section(Some("socks5"));
        let logging = ini.section(Some("logging"));

        Self {
            proxy: drover
                .and_then(|s| s.get("proxy"))
                .unwrap_or_default()
                .trim()
                .to_string(),
            udp: UdpSettings::from_section(udp),
            socks5_udp_associate: socks5
                .and_then(|s| s.get("udp_associate"))
                .map(parse_bool)
                .unwrap_or(false),
            log_level: logging.and_then(|s| s.get("level")).map(str::to_string),
            log_file_enabled: logging
                .and_then(|s| s.get("file_enabled"))
                .map(parse_bool)
                .unwrap_or(true),
            log_file: logging.and_then(|s| s.get("file")).map(str::to_string),
            log_console: logging
                .and_then(|s| s.get("console"))
                .map(parse_bool)
                .unwrap_or(false),
        }
    }

    /// Save to disk. Intentionally re-creates the file from scratch (does
    /// not preserve comments) — this matches the Pascal behavior and keeps
    /// the GUI as the authoritative source of truth.
    pub fn save(&self, path: &Path) -> Result<()> {
        let mut ini = Ini::new();

        ini.with_section(Some("drover")).set("proxy", &self.proxy);

        // SectionSetter::set returns `&mut Self`, so we deliberately keep one
        // mutable binding per section and chain `set` calls as statements
        // rather than rebinding. That avoids the "incompatible match arms"
        // class of error when a setter is sometimes called and sometimes not.
        {
            let mut udp = ini.with_section(Some("udp"));
            udp.set("strategy", self.udp.strategy.as_str());
            udp.set("prefix_delay_ms", self.udp.prefix_delay_ms.to_string());
            udp.set(
                "prefix_packets",
                udp::format_prefix_packets(&self.udp.prefix_packets),
            );
            udp.set("split_first", self.udp.split_first.to_string());
            udp.set("force_tcp_fallback", bool_str(self.udp.force_tcp_fallback));
        }

        ini.with_section(Some("socks5"))
            .set("udp_associate", bool_str(self.socks5_udp_associate));

        // Always emit the [logging] section with explicit destinations so
        // the user's choice is not lost when the GUI rewrites this file.
        {
            let mut log = ini.with_section(Some("logging"));
            log.set("level", self.log_level.as_deref().unwrap_or("info"));
            log.set("file_enabled", bool_str(self.log_file_enabled));
            log.set(
                "file",
                self.log_file.as_deref().unwrap_or(LOG_FILENAME_DEFAULT),
            );
            log.set("console", bool_str(self.log_console));
        }

        ini.write_to_file(path)
            .with_context(|| format!("failed to write {}", path.display()))
    }

    /// Parsed and validated view of `proxy`.
    pub fn proxy_value(&self) -> ProxyValue {
        ProxyValue::parse(&self.proxy)
    }
}

fn parse_bool(s: &str) -> bool {
    matches!(
        s.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

fn bool_str(b: bool) -> &'static str {
    if b {
        "true"
    } else {
        "false"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_default() {
        let tmp = std::env::temp_dir().join("redrover-test.ini");
        let opt = DroverOptions::default();
        opt.save(&tmp).unwrap();
        let loaded = DroverOptions::load(&tmp).unwrap();
        assert_eq!(loaded, opt);
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn defaults_enable_standard_logging_destination() {
        let opt = DroverOptions::default();
        assert_eq!(opt.log_level.as_deref(), Some("info"));
        assert!(opt.log_file_enabled);
        assert_eq!(opt.log_file.as_deref(), Some(LOG_FILENAME_DEFAULT));
    }

    #[test]
    fn roundtrip_can_disable_file_logging() {
        let tmp = std::env::temp_dir().join("redrover-file-logging-disabled-test.ini");
        let mut opt = DroverOptions::default();
        opt.log_file_enabled = false;
        opt.log_console = true;
        opt.save(&tmp).unwrap();
        let loaded = DroverOptions::load(&tmp).unwrap();
        assert!(!loaded.log_file_enabled);
        assert!(loaded.log_console);
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn proxy_parses() {
        let mut opt = DroverOptions::default();
        opt.proxy = "socks5://user:pass@1.2.3.4:1080".into();
        let v = opt.proxy_value();
        assert!(v.is_specified);
        assert_eq!(v.kind, ProxyKind::Socks5);
        assert_eq!(v.host, "1.2.3.4");
        assert_eq!(v.port, 1080);
        assert_eq!(v.login, "user");
        assert_eq!(v.password, "pass");
    }
}
