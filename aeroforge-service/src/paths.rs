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

    fn from_root(root: PathBuf) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
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
