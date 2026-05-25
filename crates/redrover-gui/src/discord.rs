//! Locate Discord installations on Windows.
//!
//! Mirrors `installer/Main.pas` → `FindDiscordBaseDirs` / `FindDiscordDirs`:
//! 1. Probe `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\{Discord,DiscordCanary,DiscordPTB}`
//!    → `InstallLocation`.
//! 2. Probe `HKCU\Software\Classes\Discord\shell\open\command` and extract
//!    the path up to `app-`.
//! 3. Enumerate `app-*` subdirectories of each base dir; keep the ones that
//!    contain `Discord.exe`, `DiscordCanary.exe`, or `DiscordPTB.exe`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

pub const DISCORD_FILENAMES: &[&str] = &["Discord.exe", "DiscordCanary.exe", "DiscordPTB.exe"];
pub const APP_REG_KEYS: &[&str] = &["Discord", "DiscordCanary", "DiscordPTB"];

pub fn is_discord_executable(filename: &str) -> bool {
    DISCORD_FILENAMES
        .iter()
        .any(|f| f.eq_ignore_ascii_case(filename))
}

pub fn dir_has_discord_executable(dir: &Path) -> bool {
    DISCORD_FILENAMES
        .iter()
        .any(|name| dir.join(name).is_file())
}

#[cfg(windows)]
pub fn find_discord_base_dirs() -> Result<Vec<PathBuf>> {
    use windows::core::PCWSTR;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE,
        REG_VALUE_TYPE,
    };

    fn to_wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn read_string_value(hkey: HKEY, name: &str) -> Option<String> {
        let name_w = to_wide(name);
        let mut buf = vec![0u16; 1024];
        let mut size = (buf.len() * 2) as u32;
        let mut ty = REG_VALUE_TYPE(0);
        let res = unsafe {
            RegQueryValueExW(
                hkey,
                PCWSTR(name_w.as_ptr()),
                None,
                Some(&mut ty),
                Some(buf.as_mut_ptr() as *mut u8),
                Some(&mut size),
            )
        };
        if res.is_err() {
            return None;
        }
        let chars = (size as usize / 2).saturating_sub(1);
        let s = String::from_utf16_lossy(&buf[..chars]);
        Some(s.trim_end_matches('\0').to_string())
    }

    let mut found: BTreeSet<PathBuf> = BTreeSet::new();

    for app in APP_REG_KEYS {
        let key_path = format!(
            "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{}",
            app
        );
        let key_path_w = to_wide(&key_path);
        let mut hkey = HKEY::default();
        let res = unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(key_path_w.as_ptr()),
                0,
                KEY_QUERY_VALUE,
                &mut hkey,
            )
        };
        if res.is_ok() {
            if let Some(loc) = read_string_value(hkey, "InstallLocation") {
                if !loc.is_empty() {
                    let path = PathBuf::from(loc);
                    if path.is_dir() {
                        found.insert(path);
                    }
                }
            }
            unsafe {
                let _ = RegCloseKey(hkey);
            }
        }
    }

    // Fallback: Software\Classes\Discord\shell\open\command -> "C:\...\app-X.Y.Z\Discord.exe" --args
    let key_path = to_wide("Software\\Classes\\Discord\\shell\\open\\command");
    let mut hkey = HKEY::default();
    let res = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(key_path.as_ptr()),
            0,
            KEY_QUERY_VALUE,
            &mut hkey,
        )
    };
    if res.is_ok() {
        if let Some(cmd) = read_string_value(hkey, "") {
            if let Some(base) = extract_base_from_open_command(&cmd) {
                if base.is_dir() {
                    found.insert(base);
                }
            }
        }
        unsafe {
            let _ = RegCloseKey(hkey);
        }
    }

    Ok(found.into_iter().collect())
}

#[cfg(not(windows))]
pub fn find_discord_base_dirs() -> Result<Vec<PathBuf>> {
    // On non-Windows we don't have any Discord install to find.
    // Return empty so the GUI still launches for testing.
    Ok(Vec::new())
}

fn extract_base_from_open_command(cmd: &str) -> Option<PathBuf> {
    // The shell command typically looks like:
    //   "C:\Users\Foo\AppData\Local\Discord\app-1.2.3\Discord.exe" -- "%1"
    // We want the path up to (and including) the parent of "app-...".
    let re = regex::Regex::new(r#"\A"(.+\\)app-"#).ok()?;
    let caps = re.captures(cmd)?;
    Some(PathBuf::from(caps.get(1)?.as_str()))
}

pub fn find_discord_app_dirs() -> Result<Vec<PathBuf>> {
    let bases = find_discord_base_dirs().context("registry query failed")?;
    let mut out = Vec::new();
    for base in bases {
        let Ok(entries) = std::fs::read_dir(&base) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if !name.starts_with("app-") {
                continue;
            }
            if !path.is_dir() {
                continue;
            }
            if dir_has_discord_executable(&path) {
                out.push(path);
            }
        }
    }
    Ok(out)
}

/// Pick the newest `app-X.Y.Z[.W]` directory by version order.
pub fn pick_newest(dirs: &[PathBuf]) -> Option<PathBuf> {
    let mut best: Option<(Vec<u32>, &PathBuf)> = None;
    for d in dirs {
        let Some(name) = d.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Some(version) = name.strip_prefix("app-") else {
            continue;
        };
        let parts: Vec<u32> = version
            .split('.')
            .map(|p| p.parse().unwrap_or(0))
            .collect();
        match &best {
            None => best = Some((parts, d)),
            Some((b, _)) if &parts > b => best = Some((parts, d)),
            _ => {}
        }
    }
    best.map(|(_, p)| p.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pick_newest_orders_by_version() {
        let dirs = vec![
            PathBuf::from("/x/app-1.0.0"),
            PathBuf::from("/x/app-1.10.0"),
            PathBuf::from("/x/app-2.0.0"),
            PathBuf::from("/x/app-1.9.9"),
        ];
        assert_eq!(pick_newest(&dirs).unwrap(), PathBuf::from("/x/app-2.0.0"));
    }

    #[test]
    fn extracts_base_path() {
        let cmd = r#""C:\Users\me\AppData\Local\Discord\app-1.0.0\Discord.exe" -- "%1""#;
        let base = extract_base_from_open_command(cmd).unwrap();
        assert_eq!(base, PathBuf::from(r"C:\Users\me\AppData\Local\Discord\"));
    }
}
