#![cfg(windows)]
#![windows_subsystem = "windows"]

use std::{
    env,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

fn main() {
    let log_path = log_path();
    let current_exe = match env::current_exe() {
        Ok(path) => path,
        Err(error) => {
            write_log(&log_path, &format!("failed to resolve current exe: {error}"));
            return;
        }
    };
    let current_dir = current_exe
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));

    if let Some(installed_app) = find_installed_app(&current_exe) {
        match Command::new(&installed_app).spawn() {
            Ok(child) => {
                write_log(
                    &log_path,
                    &format!("launched installed app {} pid={}", installed_app.display(), child.id()),
                );
            }
            Err(error) => write_log(
                &log_path,
                &format!("failed to launch installed app {}: {error}", installed_app.display()),
            ),
        }
        return;
    }

    let Some(setup_exe) = find_setup_exe(&current_dir) else {
        write_log(
            &log_path,
            &format!("no installed app or setup exe found from {}", current_dir.display()),
        );
        return;
    };

    match Command::new(&setup_exe).spawn() {
        Ok(child) => write_log(
            &log_path,
            &format!("launched setup {} pid={}", setup_exe.display(), child.id()),
        ),
        Err(error) => write_log(
            &log_path,
            &format!("failed to launch setup {}: {error}", setup_exe.display()),
        ),
    }
}

fn find_installed_app(current_exe: &Path) -> Option<PathBuf> {
    installed_app_candidates()
        .into_iter()
        .filter(|candidate| candidate.exists())
        .find(|candidate| !paths_equal(candidate, current_exe))
}

fn installed_app_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    for key in ["ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(root) = env::var_os(key).map(PathBuf::from) {
            candidates.push(root.join("AeroForge Control").join("aeroforge-control.exe"));
        }
    }
    candidates
}

fn find_setup_exe(current_dir: &Path) -> Option<PathBuf> {
    let mut matches = fs::read_dir(current_dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .map(|name| {
                    name.starts_with("AeroForge-Control-Setup-")
                        && name.to_ascii_lowercase().ends_with(".exe")
                })
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();
    matches.sort();
    matches.pop()
}

fn paths_equal(left: &Path, right: &Path) -> bool {
    let left = left
        .canonicalize()
        .unwrap_or_else(|_| left.to_path_buf())
        .to_string_lossy()
        .to_ascii_lowercase();
    let right = right
        .canonicalize()
        .unwrap_or_else(|_| right.to_path_buf())
        .to_string_lossy()
        .to_ascii_lowercase();
    left == right
}

fn log_path() -> PathBuf {
    env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("com.noah.aeroforgecontrol")
        .join("update-bridge.log")
}

fn write_log(path: &Path, message: &str) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{} {message}", timestamp_seconds());
    }
}

fn timestamp_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}
