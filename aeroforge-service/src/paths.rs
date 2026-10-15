use std::{
    env, fs,
    fs::OpenOptions,
    io::Write,
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
};

use crate::DynError;

#[derive(Clone, Debug)]
pub struct ServicePaths {
    pub logs_dir: PathBuf,
    pub state_dir: PathBuf,
}

impl ServicePaths {
    pub fn discover() -> Result<Self, DynError> {
        let program_data = env::var_os("ProgramData")
            .map(PathBuf::from)
            .ok_or("ProgramData is not set")?;
        let root = program_data.join("AeroForge").join("Service");
        Self::from_root(root)
    }

    pub fn discover_console() -> Result<Self, DynError> {
        if let Some(root) = env::var_os("AEROFORGE_SERVICE_ROOT").map(PathBuf::from) {
            return Self::from_root(root);
        }

        let local_app_data = env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .ok_or("LOCALAPPDATA is not set")?;
        Self::from_root(local_app_data.join("AeroForge").join("ServiceConsole"))
    }

    pub(crate) fn from_root(root: PathBuf) -> Result<Self, DynError> {
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
    pub fn harden_service_root(&self) -> Result<(), DynError> {
        restrict_acl(
            &self.service_root(),
            &[
                "*S-1-5-18:(OI)(CI)F",
                "*S-1-5-32-544:(OI)(CI)F",
                "*S-1-5-32-545:(OI)(CI)RX",
            ],
        )?;
        // Also repairs installs whose pin was written before it was locked down.
        let token = self.ipc_write_token();
        if token.exists() {
            restrict_to_system_and_admins(&token)?;
        }
        Ok(())
    }

    pub fn ipc_write_token(&self) -> PathBuf {
        self.state_dir.join("ipc-write-token.txt")
    }
}

/// The IPC write-token pin must not inherit the service root's Users:RX, or
/// any local account could read it and send hardware writes over the pipe.
pub fn restrict_to_system_and_admins(path: &Path) -> Result<(), DynError> {
    restrict_acl(path, &["*S-1-5-18:F", "*S-1-5-32-544:F"])
}

fn restrict_acl(path: &Path, grants: &[&str]) -> Result<(), DynError> {
    let output = system32_command("icacls")
        .arg(path)
        .args(["/inheritance:r", "/grant:r"])
        .args(grants)
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "icacls on {} failed: {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        )
        .into());
    }
    Ok(())
}

/// Absolute path of a Windows system directory file, e.g. `System32\powercfg.exe`.
pub fn system32_path(file_name: &str) -> PathBuf {
    let root = env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
    PathBuf::from(root).join("System32").join(file_name)
}

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Launch a system tool by absolute path. `Command::new("powercfg")` would let
/// CreateProcess search the (user-writable) application directory first.
/// Every child runs windowless; the service has no desktop to show one on.
pub fn system32_command(tool: &str) -> std::process::Command {
    // Windows PowerShell is the one tool that does not live directly in System32.
    let relative = if tool == "powershell" {
        r"WindowsPowerShell\v1.0\powershell.exe".to_string()
    } else {
        format!("{tool}.exe")
    };
    let mut command = std::process::Command::new(system32_path(&relative));
    command.creation_flags(CREATE_NO_WINDOW);
    command
}

/// A hidden Windows PowerShell running one inline script.
pub fn powershell_command(script: &str) -> std::process::Command {
    let mut command = system32_command("powershell");
    command.args([
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        script,
    ]);
    command
}

/// What a failed tool actually said: stderr, else stdout, else its exit status.
pub fn command_failure_detail(tool: &str, output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !stderr.is_empty() {
        return stderr;
    }
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !stdout.is_empty() {
        return stdout;
    }
    format!("{tool} exited with status {}", output.status)
}

/// Component logs are append-only and the periodic fan workers write several lines a
/// minute, so keep one rotated generation instead of growing without bound.
const LOG_ROTATE_BYTES: u64 = 4 * 1024 * 1024;
/// Serializes the size check and rename: two workers rotating the same log at once would
/// rename a fresh, near-empty file over the generation that was just rotated out.
static LOG_ROTATE_LOCK: Mutex<()> = Mutex::new(());

pub fn write_log_line(log_path: &Path, level: &str, message: &str) -> Result<(), DynError> {
    let line = format!(
        "{} [{}] {}\n",
        crate::workers::unix_timestamp(),
        level,
        message
    );
    let _rotate_guard = LOG_ROTATE_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if fs::metadata(log_path)
        .map(|metadata| metadata.len() >= LOG_ROTATE_BYTES)
        .unwrap_or(false)
    {
        // ponytail: single .1 generation; add numbered rotation if support ever needs more history.
        let _ = fs::rename(log_path, log_path.with_extension("log.1"));
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)?;
    file.write_all(line.as_bytes())?;
    Ok(())
}

/// Write JSON/text snapshots atomically so readers never observe a partial file.
/// Each call gets its own temp file so two writers of the same path never share one.
pub fn write_text_atomically(path: &Path, content: &str) -> Result<(), DynError> {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);

    let parent = path
        .parent()
        .ok_or("snapshot path has no parent directory")?;
    fs::create_dir_all(parent)?;

    let temp_path = parent.join(format!(
        ".{}.{}.{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("snapshot"),
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
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
fn replace_file(source: &Path, target: &Path) -> Result<(), DynError> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(Some(0)).collect()
    }

    let source_w = wide(source);
    let target_w = wide(target);
    // A replace loses briefly to another replace of the same target, or to a
    // scanner/reader holding it open, as ACCESS_DENIED or SHARING_VIOLATION;
    // both clear within milliseconds, so retry before giving up.
    let mut error = None;
    for attempt in 0..10 {
        let moved = unsafe {
            MoveFileExW(
                source_w.as_ptr(),
                target_w.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        };
        if moved != 0 {
            return Ok(());
        }
        let last = std::io::Error::last_os_error();
        let transient = matches!(last.raw_os_error(), Some(5 | 32));
        error = Some(last);
        if !transient || attempt == 9 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    let _ = fs::remove_file(source);
    Err(error.expect("replace loop ran at least once").into())
}

#[cfg(not(windows))]
fn replace_file(source: &Path, target: &Path) -> Result<(), DynError> {
    fs::rename(source, target)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_pin_acl_drops_users() {
        let temp = tempfile::tempdir().expect("tempdir");
        let token = temp.path().join("ipc-write-token.txt");
        fs::write(&token, "pin").expect("seed token");

        restrict_to_system_and_admins(&token).expect("icacls");

        // /save emits SDDL with locale-free SID aliases (SY, BA, BU, AU).
        let saved = temp.path().join("acl.txt");
        let output = system32_command("icacls")
            .arg(&token)
            .arg("/save")
            .arg(&saved)
            .output()
            .expect("icacls /save");
        assert!(output.status.success());
        let raw = fs::read(&saved).expect("read acl");
        let units: Vec<u16> = raw
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        let sddl = String::from_utf16_lossy(&units);
        assert!(sddl.contains(";;;SY)") && sddl.contains(";;;BA)"), "{sddl}");
        assert!(
            !sddl.contains(";;;BU)") && !sddl.contains(";;;AU)"),
            "{sddl}"
        );
    }

    #[test]
    fn write_log_line_rotates_once_the_cap_is_reached() {
        let temp = tempfile::tempdir().expect("tempdir");
        let log_path = temp.path().join("control-fan.log");
        fs::write(&log_path, vec![b'x'; LOG_ROTATE_BYTES as usize]).expect("seed log");

        write_log_line(&log_path, "INFO", "after rotation").expect("write");

        let rotated = temp.path().join("control-fan.log.1");
        assert_eq!(
            fs::metadata(&rotated).expect("rotated").len(),
            LOG_ROTATE_BYTES
        );
        let fresh = fs::read_to_string(&log_path).expect("fresh log");
        assert!(fresh.ends_with("[INFO] after rotation\n"));
        assert!(fresh.len() < 128);

        // A second rotation replaces the previous generation rather than failing.
        fs::write(&log_path, vec![b'y'; LOG_ROTATE_BYTES as usize]).expect("seed again");
        write_log_line(&log_path, "INFO", "second").expect("write again");
        assert!(fs::read(&rotated).expect("rotated again").starts_with(b"y"));
    }

    #[test]
    fn concurrent_atomic_writes_never_leave_a_torn_file() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("snapshot.json");
        let bodies: Vec<String> = (0..8u8)
            .map(|writer| String::from_utf8(vec![b'a' + writer; 64 * 1024]).expect("ascii"))
            .collect();

        std::thread::scope(|scope| {
            for body in &bodies {
                scope.spawn(|| {
                    for _ in 0..20 {
                        write_text_atomically(&path, body).expect("write");
                    }
                });
            }
        });

        let content = fs::read_to_string(&path).expect("read");
        assert!(
            bodies.contains(&content),
            "final file is a torn mix of writers"
        );
    }

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
