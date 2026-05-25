//! Application state and update loop.
//!
//! This mirrors `installer/Main.pas` from the Pascal project:
//! - The user picks HTTP / SOCKS5 / Direct.
//! - The user enters host/port and optional login/password.
//! - On "Install" we locate every `app-*` Discord folder via the Windows
//!   registry and drop `version.dll`, `drover.ini`, and `drover-packet.bin`
//!   into each one. "Uninstall" removes them.
//!
//! The new bits (relative to the Pascal version) are the UDP strategy
//! selector and the SOCKS5 UDP-associate toggle — they exist so the
//! ideas from `ISSUE-65-IDEAS.md` can be enabled by the user without
//! editing `drover.ini` by hand.

use std::path::PathBuf;

use iced::Task;
use redrover_config::{
    udp::{format_prefix_packets, parse_prefix_packets},
    DroverOptions, ProxyKind, ProxyValue, UdpStrategy, LOG_FILENAME_DEFAULT,
};

use crate::{discord, install};

#[derive(Debug, Clone)]
pub enum Message {
    ProxyKindSelected(ProxyKindChoice),
    HostChanged(String),
    PortChanged(String),
    AuthToggled(bool),
    LoginChanged(String),
    PasswordChanged(String),

    UdpStrategySelected(UdpStrategy),
    UdpDelayChanged(String),
    UdpPrefixPacketsChanged(String),
    UdpSplitFirstChanged(String),
    UdpForceTcpToggled(bool),
    Socks5UdpAssociateToggled(bool),
    LogLevelSelected(LogLevelChoice),
    LogFileToggled(bool),
    LogFileChanged(String),
    LogConsoleToggled(bool),

    InstallPressed,
    UninstallPressed,
    OperationFinished(Result<String, String>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProxyKindChoice {
    Http,
    Socks5,
    Direct,
}

impl ProxyKindChoice {
    pub const ALL: &'static [Self] = &[Self::Http, Self::Socks5, Self::Direct];

    pub fn label(&self) -> &'static str {
        match self {
            Self::Http => "HTTP",
            Self::Socks5 => "SOCKS5",
            Self::Direct => "Direct",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevelChoice {
    Off,
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl LogLevelChoice {
    pub const ALL: &'static [Self] = &[
        Self::Off,
        Self::Error,
        Self::Warn,
        Self::Info,
        Self::Debug,
        Self::Trace,
    ];

    fn parse(value: Option<&str>) -> Self {
        match value.unwrap_or("info").trim().to_ascii_lowercase().as_str() {
            "off" => Self::Off,
            "error" => Self::Error,
            "warn" => Self::Warn,
            "debug" => Self::Debug,
            "trace" => Self::Trace,
            _ => Self::Info,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Error => "error",
            Self::Warn => "warn",
            Self::Info => "info",
            Self::Debug => "debug",
            Self::Trace => "trace",
        }
    }
}

impl std::fmt::Display for LogLevelChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = match self {
            Self::Off => "Off",
            Self::Error => "Error",
            Self::Warn => "Warn",
            Self::Info => "Info",
            Self::Debug => "Debug",
            Self::Trace => "Trace",
        };
        f.write_str(label)
    }
}

#[derive(Debug)]
pub struct App {
    // Form state
    pub kind: ProxyKindChoice,
    pub host: String,
    pub port: String,
    pub auth: bool,
    pub login: String,
    pub password: String,

    // UDP / issue #65 controls
    pub udp_strategy: UdpStrategy,
    pub udp_delay_ms: String,
    pub udp_prefix_packets: String,
    pub udp_split_first: String,
    pub udp_force_tcp_fallback: bool,
    pub socks5_udp_associate: bool,
    pub log_level: LogLevelChoice,
    pub log_file_enabled: bool,
    pub log_file: String,
    pub log_console: bool,

    // Runtime
    pub status: Status,
    pub current_exe_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub enum Status {
    Idle,
    Working(String),
    Ok(String),
    Err(String),
}

impl App {
    pub fn new() -> (Self, Task<Message>) {
        let current_exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

        // Try to seed the form from the most recently installed drover.ini
        let opt = locate_existing_options(&current_exe_dir).unwrap_or_default();
        let proxy = opt.proxy_value();
        let udp_strategy = if opt.udp.strategy.is_gui_choice() {
            opt.udp.strategy
        } else {
            UdpStrategy::Classic
        };
        let udp_split_first =
            if matches!(udp_strategy, UdpStrategy::Split) && opt.udp.split_first < 2 {
                "2".to_string()
            } else {
                opt.udp.split_first.to_string()
            };

        let mut app = Self {
            kind: ProxyKindChoice::Direct,
            host: String::new(),
            port: String::new(),
            auth: false,
            login: String::new(),
            password: String::new(),

            udp_strategy,
            udp_delay_ms: opt.udp.prefix_delay_ms.to_string(),
            udp_prefix_packets: format_prefix_packets(&opt.udp.prefix_packets),
            udp_split_first,
            udp_force_tcp_fallback: opt.udp.force_tcp_fallback,
            socks5_udp_associate: opt.socks5_udp_associate,
            log_level: LogLevelChoice::parse(opt.log_level.as_deref()),
            log_file_enabled: opt.log_file_enabled,
            log_file: opt
                .log_file
                .unwrap_or_else(|| LOG_FILENAME_DEFAULT.to_string()),
            log_console: opt.log_console,

            status: Status::Idle,
            current_exe_dir,
        };

        if proxy.is_specified {
            // Compute scalar fields before moving the String fields out of `proxy`.
            let has_auth = proxy.has_auth();
            let port_text = if proxy.port == 0 {
                String::new()
            } else {
                proxy.port.to_string()
            };
            app.kind = match proxy.kind {
                ProxyKind::Http => ProxyKindChoice::Http,
                ProxyKind::Socks5 => ProxyKindChoice::Socks5,
            };
            app.host = proxy.host;
            app.port = port_text;
            app.auth = has_auth;
            app.login = proxy.login;
            app.password = proxy.password;
        }

        (app, Task::none())
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::ProxyKindSelected(k) => {
                self.kind = k;
                if !matches!(k, ProxyKindChoice::Socks5) {
                    self.socks5_udp_associate = false;
                }
            }
            Message::HostChanged(v) => self.host = v,
            Message::PortChanged(v) => {
                self.port = v.chars().filter(|c| c.is_ascii_digit()).collect()
            }
            Message::AuthToggled(v) => self.auth = v,
            Message::LoginChanged(v) => self.login = v,
            Message::PasswordChanged(v) => self.password = v,

            Message::UdpStrategySelected(s) => {
                self.udp_strategy = s;
                if matches!(s, UdpStrategy::Split)
                    && self.udp_split_first.parse::<u8>().unwrap_or(0) < 2
                {
                    self.udp_split_first = "2".into();
                }
            }
            Message::UdpDelayChanged(v) => {
                self.udp_delay_ms = v.chars().filter(|c| c.is_ascii_digit()).collect();
            }
            Message::UdpPrefixPacketsChanged(v) => self.udp_prefix_packets = v,
            Message::UdpSplitFirstChanged(v) => {
                self.udp_split_first = v.chars().filter(|c| c.is_ascii_digit()).collect()
            }
            Message::UdpForceTcpToggled(v) => self.udp_force_tcp_fallback = v,
            Message::Socks5UdpAssociateToggled(v) => {
                self.socks5_udp_associate = v && matches!(self.kind, ProxyKindChoice::Socks5);
            }
            Message::LogLevelSelected(v) => self.log_level = v,
            Message::LogFileToggled(v) => self.log_file_enabled = v,
            Message::LogFileChanged(v) => self.log_file = v,
            Message::LogConsoleToggled(v) => self.log_console = v,

            Message::InstallPressed => {
                if let Err(e) = self.validate() {
                    self.status = Status::Err(e);
                    return Task::none();
                }
                let opts = self.to_options();
                let exe_dir = self.current_exe_dir.clone();
                self.status = Status::Working("Installing...".into());
                return Task::perform(async move { install::install(&exe_dir, &opts) }, |r| {
                    Message::OperationFinished(r.map_err(|e| e.to_string()))
                });
            }
            Message::UninstallPressed => {
                let exe_dir = self.current_exe_dir.clone();
                self.status = Status::Working("Uninstalling...".into());
                return Task::perform(async move { install::uninstall(&exe_dir) }, |r| {
                    Message::OperationFinished(r.map_err(|e| e.to_string()))
                });
            }
            Message::OperationFinished(Ok(msg)) => self.status = Status::Ok(msg),
            Message::OperationFinished(Err(msg)) => self.status = Status::Err(msg),
        }
        Task::none()
    }

    pub fn view(&self) -> iced::Element<'_, Message> {
        crate::view::view(self)
    }

    fn validate(&self) -> Result<(), String> {
        if !matches!(self.kind, ProxyKindChoice::Direct) {
            let host = self.host.trim();
            let port: u32 = self.port.trim().parse().unwrap_or(0);
            if host.is_empty() {
                return Err("Host is empty.".into());
            }
            if !(1..=65535).contains(&port) {
                return Err("Port must be between 1 and 65535.".into());
            }
            if self.auth && (self.login.trim().is_empty() || self.password.is_empty()) {
                return Err(
                    "Login and password are required when authentication is enabled.".into(),
                );
            }
        }
        if matches!(self.udp_strategy, UdpStrategy::Split) {
            let parts: u8 = self.udp_split_first.trim().parse().unwrap_or(0);
            if !(2..=74).contains(&parts) {
                return Err("Split packet count must be between 2 and 74.".into());
            }
        }
        Ok(())
    }

    fn to_options(&self) -> DroverOptions {
        let mut opt = DroverOptions::default();

        if !matches!(self.kind, ProxyKindChoice::Direct) {
            let proxy = ProxyValue {
                is_specified: true,
                kind: match self.kind {
                    ProxyKindChoice::Http => ProxyKind::Http,
                    ProxyKindChoice::Socks5 => ProxyKind::Socks5,
                    ProxyKindChoice::Direct => unreachable!(),
                },
                login: if self.auth {
                    self.login.trim().to_string()
                } else {
                    String::new()
                },
                password: if self.auth {
                    self.password.clone()
                } else {
                    String::new()
                },
                host: self.host.trim().to_string(),
                port: self.port.trim().parse().unwrap_or(0),
            };
            opt.proxy = proxy.format_url();
        }

        opt.udp.strategy = self.udp_strategy;
        opt.udp.prefix_delay_ms = self.udp_delay_ms.parse().unwrap_or(50);
        opt.udp.prefix_packets = parse_prefix_packets(&self.udp_prefix_packets);
        opt.udp.split_first = self.udp_split_first.parse().unwrap_or(0);
        opt.udp.force_tcp_fallback = self.udp_force_tcp_fallback;
        opt.socks5_udp_associate =
            self.socks5_udp_associate && matches!(self.kind, ProxyKindChoice::Socks5);
        opt.log_level = Some(self.log_level.as_str().to_string());
        opt.log_file_enabled = self.log_file_enabled;
        opt.log_file = Some(
            if self.log_file.trim().is_empty() {
                LOG_FILENAME_DEFAULT
            } else {
                self.log_file.trim()
            }
            .to_string(),
        );
        opt.log_console = self.log_console;

        opt
    }
}

/// Look at the newest `app-*` Discord directory and read its `drover.ini`
/// to pre-fill the form, falling back to the installer's own directory.
fn locate_existing_options(exe_dir: &std::path::Path) -> Option<DroverOptions> {
    if let Ok(dirs) = discord::find_discord_app_dirs() {
        if let Some(newest) = discord::pick_newest(&dirs) {
            let candidate = newest.join(redrover_config::OPTIONS_FILENAME);
            if candidate.exists() {
                return DroverOptions::load(&candidate).ok();
            }
        }
    }
    let candidate = exe_dir.join(redrover_config::OPTIONS_FILENAME);
    DroverOptions::load(&candidate).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_app() -> App {
        App {
            kind: ProxyKindChoice::Direct,
            host: String::new(),
            port: String::new(),
            auth: false,
            login: String::new(),
            password: String::new(),
            udp_strategy: UdpStrategy::Classic,
            udp_delay_ms: "50".into(),
            udp_prefix_packets: "00, 01".into(),
            udp_split_first: "0".into(),
            udp_force_tcp_fallback: false,
            socks5_udp_associate: false,
            log_level: LogLevelChoice::Info,
            log_file_enabled: true,
            log_file: LOG_FILENAME_DEFAULT.into(),
            log_console: false,
            status: Status::Idle,
            current_exe_dir: PathBuf::new(),
        }
    }

    #[test]
    fn socks5_udp_associate_is_saved_only_for_socks5_mode() {
        let mut app = test_app();
        app.socks5_udp_associate = true;

        app.kind = ProxyKindChoice::Direct;
        assert!(!app.to_options().socks5_udp_associate);

        app.kind = ProxyKindChoice::Http;
        assert!(!app.to_options().socks5_udp_associate);

        app.kind = ProxyKindChoice::Socks5;
        assert!(app.to_options().socks5_udp_associate);
    }

    #[test]
    fn logging_destinations_and_level_are_saved_from_form() {
        let mut app = test_app();
        app.log_level = LogLevelChoice::Debug;
        app.log_file_enabled = false;
        app.log_file = "voice-debug.log".into();
        app.log_console = true;

        let options = app.to_options();
        assert_eq!(options.log_level.as_deref(), Some("debug"));
        assert!(!options.log_file_enabled);
        assert_eq!(options.log_file.as_deref(), Some("voice-debug.log"));
        assert!(options.log_console);
    }

    #[test]
    fn selecting_split_sets_a_valid_default_part_count() {
        let mut app = test_app();
        app.udp_split_first = "0".into();

        let _ = app.update(Message::UdpStrategySelected(UdpStrategy::Split));

        assert_eq!(app.udp_split_first, "2");
        assert!(app.validate().is_ok());
    }
}
