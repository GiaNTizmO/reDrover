//! Install / Uninstall logic — exact counterpart of `installer/Main.pas`
//! `btnInstallClick` / `btnUninstallClick`.
//!
//! Strategy:
//! - For each `app-*` Discord directory found via the registry, write
//!   `drover.ini` and copy `version.dll`. Also copy `drover-packet.bin`
//!   and curated strategy payloads staged under `strategies/`.
//! - Uninstall removes those files from every detected directory.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use redrover_config::{DroverOptions, DLL_FILENAME, OPTIONS_FILENAME, PACKET_FILENAME};

use crate::discord;

const STRATEGY_PAYLOAD_FILENAMES: &[&str] = &["uae-v1.bin", "uae-v2.bin"];

pub fn install(exe_dir: &Path, opts: &DroverOptions) -> Result<String> {
    let src_dll = exe_dir.join(DLL_FILENAME);
    if !src_dll.is_file() {
        bail!(
            "{} is missing next to the installer.\nBuild the C++ DLL first (see dll/README.md) \
             and drop the resulting version.dll next to redrover.exe.",
            DLL_FILENAME
        );
    }

    if is_discord_running()? {
        bail!("Discord is running. Please close it before installing.");
    }

    let dirs = discord::find_discord_app_dirs()?;
    if dirs.is_empty() {
        bail!("No Discord installation was found in the registry.");
    }

    let extras = extra_files();
    let mut errors: Vec<String> = Vec::new();

    // Save a master copy of drover.ini next to the installer too, so the
    // GUI can pre-fill the form next time.
    if let Err(e) = opts.save(&exe_dir.join(OPTIONS_FILENAME)) {
        errors.push(format!("{}: {}", exe_dir.display(), e));
    }

    for dir in &dirs {
        let dst_ini = dir.join(OPTIONS_FILENAME);
        if let Err(e) = opts.save(&dst_ini) {
            errors.push(format!("{}: {}", dst_ini.display(), e));
        }

        let dst_dll = dir.join(DLL_FILENAME);
        if let Err(e) = copy_unless_same(&src_dll, &dst_dll) {
            errors.push(format!("{}: {}", dst_dll.display(), e));
        }

        for (source, filename) in &extras {
            let src = exe_dir.join(source);
            if !src.exists() {
                continue;
            }
            let dst = dir.join(filename);
            if let Err(e) = copy_unless_same(&src, &dst) {
                errors.push(format!("{}: {}", dst.display(), e));
            }
        }
    }

    if errors.is_empty() {
        Ok(format!("Installed into {} Discord folder(s).", dirs.len()))
    } else {
        bail!("Installed with errors:\n{}", errors.join("\n"))
    }
}

pub fn uninstall(_exe_dir: &Path) -> Result<String> {
    if is_discord_running()? {
        bail!("Discord is running. Please close it before uninstalling.");
    }

    let dirs = discord::find_discord_app_dirs()?;
    if dirs.is_empty() {
        return Ok("Nothing to uninstall.".into());
    }

    let mut removed = 0;
    let mut errors = Vec::new();
    for dir in &dirs {
        for filename in [OPTIONS_FILENAME, DLL_FILENAME, PACKET_FILENAME]
            .into_iter()
            .chain(STRATEGY_PAYLOAD_FILENAMES.iter().copied())
        {
            let path = dir.join(filename);
            if path.exists() {
                match fs::remove_file(&path) {
                    Ok(()) => removed += 1,
                    Err(e) => errors.push(format!("{}: {}", path.display(), e)),
                }
            }
        }
    }

    if errors.is_empty() {
        Ok(format!("Uninstalled {} file(s).", removed))
    } else {
        bail!("Uninstalled with errors:\n{}", errors.join("\n"))
    }
}

fn copy_unless_same(src: &Path, dst: &Path) -> Result<()> {
    if normalize(src) == normalize(dst) {
        return Ok(());
    }
    fs::copy(src, dst)?;
    Ok(())
}

fn normalize(p: &Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}

/// Optional payload files copied alongside `version.dll`.
///
/// Build output keeps curated strategy files in a `strategies/` directory,
/// while the injected DLL reads them from the Discord executable directory.
fn extra_files() -> Vec<(PathBuf, &'static str)> {
    std::iter::once((PathBuf::from(PACKET_FILENAME), PACKET_FILENAME))
        .chain(
            STRATEGY_PAYLOAD_FILENAMES
                .iter()
                .copied()
                .map(|filename| (PathBuf::from("strategies").join(filename), filename)),
        )
        .collect()
}

#[cfg(windows)]
fn is_discord_running() -> Result<bool> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)?;
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut found = false;
        if Process32FirstW(snap, &mut entry).is_ok() {
            loop {
                let end = entry
                    .szExeFile
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(entry.szExeFile.len());
                let name = String::from_utf16_lossy(&entry.szExeFile[..end]);
                if discord::is_discord_executable(&name) {
                    found = true;
                    break;
                }
                if Process32NextW(snap, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
        Ok(found)
    }
}

#[cfg(not(windows))]
fn is_discord_running() -> Result<bool> {
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strategy_payloads_are_flattened_for_runtime_lookup() {
        assert_eq!(
            extra_files(),
            vec![
                (PathBuf::from(PACKET_FILENAME), PACKET_FILENAME),
                (PathBuf::from("strategies").join("uae-v1.bin"), "uae-v1.bin"),
                (PathBuf::from("strategies").join("uae-v2.bin"), "uae-v2.bin"),
            ]
        );
    }
}
