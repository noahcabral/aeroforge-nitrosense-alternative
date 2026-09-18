use std::{
    env,
    ffi::OsStr,
    fs::{self, OpenOptions},
    io::{BufRead, BufReader, Write},
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::models::{
    CapabilitySnapshot, CustomPowerBaseId, FanCurveSet, FanProfileId, GpuTuningState,
    LiveControlSnapshot, PowerProfileId, ProcessorStateSettings, ServiceStatus,
    ServiceWorkerStatus, TelemetrySnapshot,
};

const PIPE_PATH: &str = r"\\.\pipe\AeroForgeService";
const SERVICE_NAME: &str = "AeroForgeService";
const APP_CONFIG_DIR: &str = "com.noah.aeroforgecontrol";
const IPC_WRITE_TOKEN_FILE: &str = "ipc-write-token.txt";
const FRESH_SUPERVISOR_MAX_AGE_SECONDS: u64 = 15;
const FRESH_PERIODIC_WORKER_MAX_AGE_SECONDS: u64 = 15;
const FRESH_CACHED_SNAPSHOT_MAX_AGE_SECONDS: u64 = 20;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const CRITICAL_WORKERS: &[&str] = &["control-worker", "ipc-worker"];

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CachedSupervisorSnapshot {
    service: String,
    worker_count: usize,
    updated_at_unix: Option<u64>,
    workers: Vec<ServiceWorkerStatus>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum PipeRequest {
    GetServiceStatus,
    GetCapabilities,
    GetControlSnapshot,
    GetTelemetrySnapshot,
    RestoreStartupState {
        write_token: String,
    },
    ApplyPowerProfile {
        payload: ApplyPowerProfileRequest,
        write_token: String,
    },
    ApplyGpuTuning {
        payload: ApplyGpuTuningRequest,
        write_token: String,
    },
    ApplyFanProfile {
        payload: ApplyFanProfileRequest,
        write_token: String,
    },
    ApplyCustomFanCurves {
        payload: ApplyCustomFanCurvesRequest,
        write_token: String,
    },
    StartFanSpeedCalibration {
        write_token: String,
    },
    CancelFanSpeedCalibration {
        write_token: String,
    },
    ApplyBootLogo {
        payload: ApplyBootLogoRequest,
        write_token: String,
    },
    ApplySmartCharging {
        payload: ApplySmartChargeRequest,
        write_token: String,
    },
    ApplyTelemetrySettings {
        payload: ApplyTelemetrySettingsRequest,
        write_token: String,
    },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum PipeResponse {
    Ok { payload: Value },
    Error { message: String },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ApplyPowerProfileRequest {
    profile_id: PowerProfileId,
    processor_state: ProcessorStateSettings,
    custom_base_profile: Option<CustomPowerBaseId>,
    processor_state_control_enabled: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ApplyGpuTuningRequest {
    tuning: GpuTuningState,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ApplyFanProfileRequest {
    profile_id: FanProfileId,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ApplyCustomFanCurvesRequest {
    curves: FanCurveSet,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ApplyBootLogoRequest {
    image_path: String,
    original_filename: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ApplySmartChargeRequest {
    enabled: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ApplyTelemetrySettingsRequest {
    nvidia_telemetry_enabled: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppliedPowerProfilePayload {
    pub profile_id: PowerProfileId,
    pub processor_state: ProcessorStateSettings,
    #[serde(default = "default_true")]
    pub processor_state_control_enabled: bool,
    #[serde(default)]
    pub detail: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppliedGpuTuningPayload {
    pub tuning: GpuTuningState,
    pub applied_at_unix: u64,
    pub detail: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppliedFanControlPayload {
    pub profile_id: FanProfileId,
    pub curves: Option<FanCurveSet>,
    pub applied_at_unix: u64,
    pub detail: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppliedBootLogoPayload {
    pub applied_at_unix: u64,
    pub detail: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppliedSmartChargePayload {
    pub enabled: bool,
    pub battery_healthy: u8,
    pub applied_at_unix: u64,
    pub detail: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppliedTelemetrySettingsPayload {
    pub nvidia_telemetry_enabled: bool,
    pub detail: String,
}

pub fn fetch_cached_service_status(pipe_error: &str) -> ServiceStatus {
    build_cached_service_status(pipe_error, false)
}

pub fn fetch_fast_service_status() -> ServiceStatus {
    build_cached_service_status(
        "Using cached service supervisor snapshot for fast UI polling.",
        true,
    )
}

fn build_cached_service_status(detail_seed: &str, trust_fresh_snapshot: bool) -> ServiceStatus {
    let state_dir = service_state_dir();
    let supervisor_file = supervisor_file_path();
    let cached_snapshot = fs::read_to_string(&supervisor_file)
        .ok()
        .and_then(|raw| serde_json::from_str::<CachedSupervisorSnapshot>(&raw).ok());

    let (service_name, worker_count, updated_at_unix, workers) =
        if let Some(snapshot) = cached_snapshot {
            (
                snapshot.service,
                snapshot.worker_count,
                snapshot.updated_at_unix,
                snapshot.workers,
            )
        } else {
            (SERVICE_NAME.into(), 0, None, Vec::new())
        };

    let fresh = trust_fresh_snapshot && supervisor_is_fresh(updated_at_unix);
    let worker_problem = critical_worker_problem(&workers);
    let connected = fresh && worker_problem.is_none();
    let missing_pipe_detail = if fresh {
        if let Some(problem) = &worker_problem {
            format!(
                "AeroForge service supervisor is fresh, but service controls are degraded: {problem}. Snapshot: {}.",
                supervisor_file.display()
            )
        } else {
            format!(
                "Loaded fresh AeroForge service supervisor snapshot from {}.",
                supervisor_file.display()
            )
        }
    } else if detail_seed.contains("os error 2") {
        format!(
            "{SERVICE_NAME} is not installed or is not running. Install AeroForge with the setup installer, or start {SERVICE_NAME}. Raw pipe error: {detail_seed}"
        )
    } else {
        format!("Service unavailable: {detail_seed}")
    };

    let detail = if fresh {
        missing_pipe_detail
    } else if supervisor_file.exists() {
        format!(
            "{missing_pipe_detail}. Loaded cached supervisor snapshot from {}.",
            supervisor_file.display()
        )
    } else {
        missing_pipe_detail
    };

    ServiceStatus {
        connected,
        pipe_name: PIPE_PATH.into(),
        service_name,
        version: None,
        state_dir: Some(state_dir.display().to_string()),
        supervisor_file: Some(supervisor_file.display().to_string()),
        worker_count,
        updated_at_unix,
        workers,
        detail,
    }
}

fn supervisor_is_fresh(updated_at_unix: Option<u64>) -> bool {
    let Some(updated_at_unix) = updated_at_unix else {
        return false;
    };
    let Ok(now) = SystemTime::now().duration_since(UNIX_EPOCH) else {
        return false;
    };

    now.as_secs()
        .checked_sub(updated_at_unix)
        .map(|age| age <= FRESH_SUPERVISOR_MAX_AGE_SECONDS)
        .unwrap_or(false)
}

pub fn fetch_service_status() -> Result<ServiceStatus, Box<dyn std::error::Error + Send + Sync>> {
    let payload = request(PipeRequest::GetServiceStatus)?;
    Ok(serde_json::from_value::<ServiceStatus>(payload)?)
}

pub fn fetch_capabilities() -> Result<CapabilitySnapshot, Box<dyn std::error::Error + Send + Sync>>
{
    let payload = request(PipeRequest::GetCapabilities)?;
    Ok(serde_json::from_value::<CapabilitySnapshot>(payload)?)
}

pub fn fetch_cached_capabilities(
) -> Result<CapabilitySnapshot, Box<dyn std::error::Error + Send + Sync>> {
    let path = capabilities_file_path();
    let (raw, updated_at_unix) = read_fresh_cached_snapshot(&path)?;
    let mut snapshot = serde_json::from_str::<CapabilitySnapshot>(&raw)?;
    snapshot.cache_updated_at_unix = Some(updated_at_unix);
    Ok(snapshot)
}

pub fn fetch_telemetry() -> Result<TelemetrySnapshot, Box<dyn std::error::Error + Send + Sync>> {
    let payload = request(PipeRequest::GetTelemetrySnapshot)?;
    Ok(serde_json::from_value::<TelemetrySnapshot>(payload)?)
}

pub fn fetch_cached_telemetry(
) -> Result<TelemetrySnapshot, Box<dyn std::error::Error + Send + Sync>> {
    let path = telemetry_file_path();
    let (raw, updated_at_unix) = read_fresh_cached_snapshot(&path)?;
    let mut snapshot = serde_json::from_str::<TelemetrySnapshot>(&raw)?;
    snapshot.cache_updated_at_unix = Some(updated_at_unix);
    Ok(snapshot)
}

pub fn fetch_cached_live_controls(
) -> Result<LiveControlSnapshot, Box<dyn std::error::Error + Send + Sync>> {
    let path = control_file_path();
    let (raw, updated_at_unix) = read_fresh_cached_snapshot(&path)?;
    let mut snapshot = serde_json::from_str::<LiveControlSnapshot>(&raw)?;
    snapshot.cache_updated_at_unix = Some(updated_at_unix);
    Ok(snapshot)
}

pub fn fetch_live_controls() -> Result<LiveControlSnapshot, Box<dyn std::error::Error + Send + Sync>>
{
    let payload = request(PipeRequest::GetControlSnapshot)?;
    Ok(serde_json::from_value::<LiveControlSnapshot>(payload)?)
}

pub fn restore_startup_state() -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let payload = request(PipeRequest::RestoreStartupState {
        write_token: load_or_create_write_token()?,
    })?;
    Ok(payload
        .get("detail")
        .and_then(Value::as_str)
        .unwrap_or("Restored saved AeroForge state on manual app launch.")
        .to_string())
}

pub fn apply_power_profile(
    profile_id: PowerProfileId,
    processor_state: ProcessorStateSettings,
    custom_base_profile: Option<CustomPowerBaseId>,
    processor_state_control_enabled: bool,
) -> Result<AppliedPowerProfilePayload, Box<dyn std::error::Error + Send + Sync>> {
    let payload = request(PipeRequest::ApplyPowerProfile {
        payload: ApplyPowerProfileRequest {
            profile_id,
            processor_state,
            custom_base_profile,
            processor_state_control_enabled,
        },
        write_token: load_or_create_write_token()?,
    })?;
    Ok(serde_json::from_value::<AppliedPowerProfilePayload>(
        payload,
    )?)
}

pub fn apply_gpu_tuning(
    tuning: GpuTuningState,
) -> Result<AppliedGpuTuningPayload, Box<dyn std::error::Error + Send + Sync>> {
    let payload = request(PipeRequest::ApplyGpuTuning {
        payload: ApplyGpuTuningRequest { tuning },
        write_token: load_or_create_write_token()?,
    })?;
    Ok(serde_json::from_value::<AppliedGpuTuningPayload>(payload)?)
}

pub fn apply_fan_profile(
    profile_id: FanProfileId,
) -> Result<AppliedFanControlPayload, Box<dyn std::error::Error + Send + Sync>> {
    let payload = request(PipeRequest::ApplyFanProfile {
        payload: ApplyFanProfileRequest { profile_id },
        write_token: load_or_create_write_token()?,
    })?;
    Ok(serde_json::from_value::<AppliedFanControlPayload>(payload)?)
}

pub fn apply_custom_fan_curves(
    curves: FanCurveSet,
) -> Result<AppliedFanControlPayload, Box<dyn std::error::Error + Send + Sync>> {
    let payload = request(PipeRequest::ApplyCustomFanCurves {
        payload: ApplyCustomFanCurvesRequest { curves },
        write_token: load_or_create_write_token()?,
    })?;
    Ok(serde_json::from_value::<AppliedFanControlPayload>(payload)?)
}

pub fn start_fan_speed_calibration(
) -> Result<super::models::FanSpeedCalibrationSnapshot, Box<dyn std::error::Error + Send + Sync>> {
    let payload = request(PipeRequest::StartFanSpeedCalibration {
        write_token: load_or_create_write_token()?,
    })?;
    Ok(serde_json::from_value::<
        super::models::FanSpeedCalibrationSnapshot,
    >(payload)?)
}

pub fn cancel_fan_speed_calibration(
) -> Result<super::models::FanSpeedCalibrationSnapshot, Box<dyn std::error::Error + Send + Sync>> {
    let payload = request(PipeRequest::CancelFanSpeedCalibration {
        write_token: load_or_create_write_token()?,
    })?;
    Ok(serde_json::from_value::<
        super::models::FanSpeedCalibrationSnapshot,
    >(payload)?)
}

pub fn apply_boot_logo(
    image_path: String,
    original_filename: Option<String>,
) -> Result<AppliedBootLogoPayload, Box<dyn std::error::Error + Send + Sync>> {
    let payload = request(PipeRequest::ApplyBootLogo {
        payload: ApplyBootLogoRequest {
            image_path,
            original_filename,
        },
        write_token: load_or_create_write_token()?,
    })?;
    Ok(serde_json::from_value::<AppliedBootLogoPayload>(payload)?)
}

pub fn apply_smart_charging(
    enabled: bool,
) -> Result<AppliedSmartChargePayload, Box<dyn std::error::Error + Send + Sync>> {
    let payload = request(PipeRequest::ApplySmartCharging {
        payload: ApplySmartChargeRequest { enabled },
        write_token: load_or_create_write_token()?,
    })?;
    Ok(serde_json::from_value::<AppliedSmartChargePayload>(
        payload,
    )?)
}

pub fn apply_telemetry_settings(
    nvidia_telemetry_enabled: bool,
) -> Result<AppliedTelemetrySettingsPayload, Box<dyn std::error::Error + Send + Sync>> {
    let payload = request(PipeRequest::ApplyTelemetrySettings {
        payload: ApplyTelemetrySettingsRequest {
            nvidia_telemetry_enabled,
        },
        write_token: load_or_create_write_token()?,
    })?;
    Ok(serde_json::from_value::<AppliedTelemetrySettingsPayload>(
        payload,
    )?)
}

/// Longest a single service request may take before the UI gets an error instead
/// of a spinner. Applies (fan, power, boot logo) finish in seconds; only a wedged
/// handler (stalled WMI/driver call) ever reaches this.
const PIPE_REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

fn request(command: PipeRequest) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
    // A named pipe opened as a File has no read timeout, so run the blocking
    // exchange on its own thread and give up waiting for it. A wedged thread is
    // released when the service answers or restarts (the pipe breaks on both).
    let (tx, rx) = std::sync::mpsc::channel();
    thread::Builder::new()
        .name("aeroforge-pipe-request".into())
        .spawn(move || {
            let _ = tx.send(request_blocking(command));
        })?;
    match rx.recv_timeout(PIPE_REQUEST_TIMEOUT) {
        Ok(result) => result,
        Err(_) => Err(format!(
            "AeroForge service did not answer within {}s. It may be stuck; restarting the AeroForge service clears it.",
            PIPE_REQUEST_TIMEOUT.as_secs()
        )
        .into()),
    }
}

fn request_blocking(
    command: PipeRequest,
) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
    let mut pipe = open_pipe_with_retry()?;
    let serialized = serde_json::to_string(&command)?;
    pipe.write_all(serialized.as_bytes())?;
    pipe.write_all(b"\n")?;
    pipe.flush()?;

    let mut reader = BufReader::new(pipe);
    let mut line = String::new();
    reader.read_line(&mut line)?;

    if line.trim().is_empty() {
        return Err("Named pipe returned an empty response".into());
    }

    match serde_json::from_str::<PipeResponse>(&line)? {
        PipeResponse::Ok { payload } => Ok(payload),
        PipeResponse::Error { message } => Err(message.into()),
    }
}

fn load_or_create_write_token() -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let path = desktop_write_token_path();
    if let Ok(existing) = fs::read_to_string(&path) {
        let token = existing.trim().to_string();
        if is_valid_write_token(&token) {
            return Ok(token);
        }
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let token = generate_write_token()?;
    write_token_atomically(&path, &token)?;
    Ok(token)
}

fn desktop_write_token_path() -> PathBuf {
    env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Users\Default\AppData\Roaming"))
        .join(APP_CONFIG_DIR)
        .join(IPC_WRITE_TOKEN_FILE)
}

fn write_token_atomically(
    path: &Path,
    token: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let temp = path.with_extension("tmp");
    fs::write(&temp, token)?;
    if path.exists() {
        fs::remove_file(path)?;
    }
    fs::rename(&temp, path)?;
    // Best-effort: lock the token down to the current user so other local
    // accounts cannot read the pipe write secret from a shared machine profile.
    let _ = restrict_token_file_acl(path);
    Ok(())
}

fn restrict_token_file_acl(path: &Path) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let username = env::var("USERNAME").unwrap_or_else(|_| "Users".into());
    let grant = format!("{username}:F");
    let output = Command::new("icacls")
        .creation_flags(CREATE_NO_WINDOW)
        .args([
            path.as_os_str(),
            OsStr::new("/inheritance:r"),
            OsStr::new("/grant:r"),
            OsStr::new(&grant),
            OsStr::new("/grant:r"),
            OsStr::new("*S-1-5-18:F"),
            OsStr::new("/grant:r"),
            OsStr::new("*S-1-5-32-544:F"),
        ])
        .output()?;

    if !output.status.success() {
        return Err(format!(
            "icacls failed to restrict {}: {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }

    Ok(())
}

fn generate_write_token() -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    use windows_sys::Win32::Security::Cryptography::{
        BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG,
    };

    let mut bytes = [0u8; 32];
    let status = unsafe {
        BCryptGenRandom(
            std::ptr::null_mut(),
            bytes.as_mut_ptr(),
            bytes.len() as u32,
            BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        )
    };
    if status != 0 {
        return Err(format!("BCryptGenRandom failed with status 0x{status:08X}").into());
    }

    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn is_valid_write_token(token: &str) -> bool {
    token.len() == 64 && token.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn open_pipe_with_retry() -> Result<std::fs::File, Box<dyn std::error::Error + Send + Sync>> {
    let mut last_error: Option<std::io::Error> = None;
    let mut recovery_attempted = false;

    for attempt in 0..20 {
        match OpenOptions::new().read(true).write(true).open(PIPE_PATH) {
            Ok(pipe) => return Ok(pipe),
            Err(error) => {
                if !recovery_attempted && attempt >= 2 && should_try_service_recovery(&error) {
                    recovery_attempted = true;
                    let _ = ensure_service_running();
                }
                last_error = Some(error);
                thread::sleep(Duration::from_millis(100));
            }
        }
    }

    Err(last_error
        .unwrap_or_else(|| std::io::Error::other("Failed to open named pipe"))
        .into())
}

fn read_fresh_cached_snapshot(
    path: &Path,
) -> Result<(String, u64), Box<dyn std::error::Error + Send + Sync>> {
    let updated_at_unix = file_modified_unix(path)?;
    let age = current_unix()?.saturating_sub(updated_at_unix);
    if age > FRESH_CACHED_SNAPSHOT_MAX_AGE_SECONDS {
        return Err(format!(
            "Cached service snapshot {} is stale: {age}s old.",
            path.display()
        )
        .into());
    }

    Ok((fs::read_to_string(path)?, updated_at_unix))
}

fn file_modified_unix(path: &Path) -> Result<u64, Box<dyn std::error::Error + Send + Sync>> {
    Ok(fs::metadata(path)?
        .modified()?
        .duration_since(UNIX_EPOCH)?
        .as_secs())
}

fn current_unix() -> Result<u64, Box<dyn std::error::Error + Send + Sync>> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())
}

pub fn ensure_service_running() -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let worker_problem = cached_supervisor_worker_problem();
    let service_state = query_service_state()?;

    match (service_state.as_deref(), worker_problem) {
        (Some("RUNNING"), Some(problem)) => {
            stop_service_best_effort()?;
            wait_for_service_state("STOPPED", Duration::from_secs(8)).ok();
            start_service()?;
            wait_for_service_state("RUNNING", Duration::from_secs(12))?;
            Ok(format!(
                "Restarted {SERVICE_NAME} because its worker state was unhealthy: {problem}."
            ))
        }
        (Some("RUNNING"), None) => Ok(format!("{SERVICE_NAME} is already running.")),
        (Some(_), _) => {
            start_service()?;
            wait_for_service_state("RUNNING", Duration::from_secs(12))?;
            Ok(format!("Started {SERVICE_NAME}."))
        }
        (None, _) => Err(format!("{SERVICE_NAME} is not installed.").into()),
    }
}

fn should_try_service_recovery(error: &std::io::Error) -> bool {
    if cached_supervisor_worker_problem().is_some() {
        return true;
    }

    matches!(
        error.kind(),
        std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
    ) || matches!(
        error.raw_os_error(),
        Some(2) | Some(109) | Some(121) | Some(231) | Some(232) | Some(233)
    )
}

fn cached_supervisor_worker_problem() -> Option<String> {
    let raw = fs::read_to_string(supervisor_file_path()).ok()?;
    let snapshot = serde_json::from_str::<CachedSupervisorSnapshot>(&raw).ok()?;
    critical_worker_problem(&snapshot.workers)
}

fn critical_worker_problem(workers: &[ServiceWorkerStatus]) -> Option<String> {
    let now_unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();

    for worker_name in CRITICAL_WORKERS {
        let Some(worker) = workers.iter().find(|worker| worker.name == *worker_name) else {
            return Some(format!(
                "{worker_name} is missing from the supervisor snapshot"
            ));
        };
        let state = worker.state.trim().to_ascii_lowercase();
        if !matches!(state.as_str(), "running" | "starting") {
            return Some(format!(
                "{} is {}{}",
                worker.name,
                worker.state,
                worker
                    .last_error
                    .as_deref()
                    .map(|error| format!(" ({error})"))
                    .unwrap_or_default()
            ));
        }

        if worker.interval_seconds > 0 {
            let max_age = FRESH_PERIODIC_WORKER_MAX_AGE_SECONDS
                .max(worker.interval_seconds.saturating_mul(4));
            let age = now_unix.saturating_sub(worker.last_update_unix);
            if age > max_age {
                return Some(format!(
                    "{} heartbeat is stale: last update was {age}s ago, expected within {max_age}s",
                    worker.name
                ));
            }
        }
    }

    None
}

fn query_service_state() -> Result<Option<String>, Box<dyn std::error::Error + Send + Sync>> {
    let output = sc_output(["query", SERVICE_NAME])?;
    let text = String::from_utf8_lossy(&output.stdout).to_string()
        + &String::from_utf8_lossy(&output.stderr);
    if text.contains("does not exist") || text.contains("1060") {
        return Ok(None);
    }
    if text.contains("RUNNING") {
        return Ok(Some("RUNNING".into()));
    }
    if text.contains("STOPPED") {
        return Ok(Some("STOPPED".into()));
    }
    if text.contains("START_PENDING") {
        return Ok(Some("START_PENDING".into()));
    }
    if text.contains("STOP_PENDING") {
        return Ok(Some("STOP_PENDING".into()));
    }
    Ok(Some("UNKNOWN".into()))
}

fn start_service() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let output = sc_output(["start", SERVICE_NAME])?;
    if output.status.success() {
        return Ok(());
    }

    let text = String::from_utf8_lossy(&output.stdout).to_string()
        + &String::from_utf8_lossy(&output.stderr);
    if text.contains("1056") || text.contains("already been started") {
        return Ok(());
    }

    Err(format!("sc.exe start {SERVICE_NAME} failed: {}", text.trim()).into())
}

fn stop_service_best_effort() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let output = sc_output(["stop", SERVICE_NAME])?;
    if output.status.success() {
        return Ok(());
    }

    let text = String::from_utf8_lossy(&output.stdout).to_string()
        + &String::from_utf8_lossy(&output.stderr);
    if text.contains("1062") || text.contains("has not been started") {
        return Ok(());
    }

    Err(format!("sc.exe stop {SERVICE_NAME} failed: {}", text.trim()).into())
}

fn wait_for_service_state(
    target: &str,
    timeout: Duration,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let started = std::time::Instant::now();
    while started.elapsed() <= timeout {
        if query_service_state()?.as_deref() == Some(target) {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(250));
    }
    Err(format!("{SERVICE_NAME} did not reach {target} within {:?}", timeout).into())
}

fn sc_output<const N: usize>(
    args: [&str; N],
) -> Result<std::process::Output, Box<dyn std::error::Error + Send + Sync>> {
    Ok(Command::new("sc.exe")
        .creation_flags(CREATE_NO_WINDOW)
        .args(args)
        .output()?)
}

fn service_state_dir() -> PathBuf {
    env::var_os("ProgramData")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"))
        .join("AeroForge")
        .join("Service")
        .join("state")
}

fn telemetry_file_path() -> PathBuf {
    service_state_dir().join("telemetry.json")
}

fn control_file_path() -> PathBuf {
    service_state_dir().join("control.json")
}

fn capabilities_file_path() -> PathBuf {
    service_state_dir().join("capabilities.json")
}

fn supervisor_file_path() -> PathBuf {
    service_state_dir().join("supervisor.json")
}
