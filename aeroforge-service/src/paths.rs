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
        let logs_dir = root.join("logs");
        let state_dir = root.join("state");

        fs::create_dir_all(&logs_dir)?;
        fs::create_dir_all(&state_dir)?;

        let _ = root;

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

fn timestamp_string() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    now.to_string()
}
