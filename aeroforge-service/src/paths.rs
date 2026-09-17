use std::{
    env, fs,
    fs::OpenOptions,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug)]
pub struct ServicePaths {
    pub logs_dir: PathBuf,
    pub state_dir: PathBuf,
}

impl ServicePaths {
    pub fn discover() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let program_data = env::var_os("ProgramData")
            .map(PathBuf::from)
            .ok_or("ProgramData is not set")?;
        let root = program_data.join("AeroForge").join("Service");
        Self::from_root(root)
    }

    pub fn discover_console() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        if let Some(root) = env::var_os("AEROFORGE_SERVICE_ROOT").map(PathBuf::from) {
            return Self::from_root(root);
        }

        let local_app_data = env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .ok_or("LOCALAPPDATA is not set")?;
        Self::from_root(local_app_data.join("AeroForge").join("ServiceConsole"))
    }

    pub(crate) fn from_root(root: PathBuf) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let logs_dir = root.join("logs");
        let state_dir = root.join("state");

        fs::create_dir_all(&logs_dir)?;
        fs::create_dir_all(&state_dir)?;

        Ok(Self {
            logs_dir,
            state_dir,
        })
    }

    pub fn service_log(&self) -> PathBuf {
        self.logs_dir.join("service.log")
    }

    pub fn component_log(&self, component_name: &str) -> PathBuf {
        self.logs_dir.join(format!("{component_name}.log"))
    }

    pub fn supervisor_snapshot(&self) -> PathBuf {
        self.state_dir.join("supervisor.json")
    }

    pub fn worker_snapshot(&self, worker_name: &str) -> PathBuf {
        self.state_dir.join(format!("{worker_name}.json"))
    }

    pub fn service_root(&self) -> PathBuf {
        self.state_dir
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| self.state_dir.clone())
    }

    /// Restrict the service directory to SYSTEM/Administrators (full) and Users
    /// (read/execute). ProgramData inherits a Users create-file ACE, and both
    /// CreateProcess and LoadLibrary search the application directory first, so
    /// without this any local user could drop a `powershell.exe` next to the
    /// service binary and have it run as SYSTEM.
    pub fn harden_service_root(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let root = self.service_root();
        let output = system32_command("icacls")
            .arg(&root)
            .args(["/inheritance:r", "/grant:r"])
            .args(["*S-1-5-18:(OI)(CI)F", "*S-1-5-32-544:(OI)(CI)F", "*S-1-5-32-545:(OI)(CI)RX"])
            .output()?;
        if !output.status.success() {
            return Err(format!(
                "icacls on {} failed: {}",
                root.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            )
            .into());
        }
        Ok(())
    }
}

/// Absolute path of a Windows system directory file, e.g. `System32\powercfg.exe`.
pub fn system32_path(file_name: &str) -> PathBuf {
    let root = env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
    PathBuf::from(root).join("System32").join(file_name)
}

/// Launch a system tool by absolute path. `Command::new("powercfg")` would let
/// CreateProcess search the (user-writable) application directory first.
pub fn system32_command(tool: &str) -> std::process::Command {
    // Windows PowerShell is the one tool that does not live directly in System32.
    let relative = if tool == "powershell" {
        r"WindowsPowerShell\v1.0\powershell.exe".to_string()
    } else {
        format!("{tool}.exe")
    };
    std::process::Command::new(system32_path(&relative))
}

pub fn write_log_line(
    log_path: &Path,
    level: &str,
    message: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let line = format!("{} [{}] {}\n", timestamp_string(), level, message);
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)?;
    file.write_all(line.as_bytes())?;
    Ok(())
}

/// Write JSON/text snapshots atomically so readers never observe a partial file.
pub fn write_text_atomically(
    path: &Path,
    content: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let parent = path.parent().ok_or("snapshot path has no parent directory")?;
    fs::create_dir_all(parent)?;

    let temp_path = parent.join(format!(
        ".{}.{}.{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("snapshot"),
        std::process::id(),
        timestamp_string()
    ));

    {
        let mut file = fs::File::create(&temp_path)?;
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
    }

    replace_file(&temp_path, path)?;
    Ok(())
}

#[cfg(windows)]
fn replace_file(
    source: &Path,
    target: &Path,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(Some(0)).collect()
    }

    let source_w = wide(source);
    let target_w = wide(target);
    let moved = unsafe {
        MoveFileExW(
            source_w.as_ptr(),
            target_w.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };

    if moved == 0 {
        let error = std::io::Error::last_os_error();
        let _ = fs::remove_file(source);
        return Err(error.into());
    }

    Ok(())
}

#[cfg(not(windows))]
fn replace_file(
    source: &Path,
    target: &Path,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    fs::rename(source, target)?;
    Ok(())
}

fn timestamp_string() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    now.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_system_tool_the_service_launches_exists_at_its_absolute_path() {
        for tool in ["powershell", "powercfg", "reg", "icacls"] {
            let program = system32_command(tool).get_program().to_os_string();
            assert!(
                Path::new(&program).is_file(),
                "{tool} resolved to {} which does not exist",
                program.to_string_lossy()
            );
        }
        // nvidia-smi and nvapi64.dll are driver-provided; only check the location shape.
        assert!(system32_path("nvapi64.dll").ends_with(r"System32\nvapi64.dll"));
    }
}
