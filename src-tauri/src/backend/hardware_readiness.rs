use std::{
    env,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        OnceLock,
    },
};

use super::{
    models::{
        AcerControlReadiness, HardwareReadinessSnapshot, RepairLaunchResult, ServiceHealResult,
        ServiceStartResult, WindowsServiceReadiness, WmiReadiness,
    },
    process::{hidden_command, hidden_powershell},
};

const AEROFORGE_SERVICE: &str = "AeroForgeService";
const WMI_SERVICE: &str = "Winmgmt";
const HEALER_TASK_NAME: &str = "AeroForge Service Healer";
const INSTALLER_SCRIPT: &str = "Install-AeroForgeBundledService.ps1";
const SERVICE_EXE: &str = "aeroforge-service.exe";
const INSTALLER_LOG: &str = r"C:\ProgramData\AeroForge\Service\logs\installer-service.log";

pub fn snapshot() -> HardwareReadinessSnapshot {
    let service = read_service_readiness(AEROFORGE_SERVICE);
    let wmi_service = read_service_readiness(WMI_SERVICE);
    let wmi_disabled = wmi_service
        .start_type
        .as_deref()
        .map(is_disabled_start_type)
        .unwrap_or(false);
    let wmi = WmiReadiness {
        service: wmi_service.clone(),
        disabled: wmi_disabled,
        needs_repair: !wmi_service.running || wmi_disabled,
        detail: if wmi_disabled {
            "Windows Management Instrumentation is disabled. Acer WMI fan and firmware power controls cannot work until Winmgmt is enabled and running.".into()
        } else if !wmi_service.running {
            "Windows Management Instrumentation is not running. Acer WMI fan and firmware power controls are blocked until Winmgmt starts.".into()
        } else {
            "Windows Management Instrumentation is running.".into()
        },
    };

    static IDENTITY: OnceLock<RegistryHardwareIdentity> = OnceLock::new();
    let identity = IDENTITY
        .get_or_init(read_registry_hardware_identity)
        .clone();
    let acer_wmi_present = if wmi.service.running {
        acer_gaming_wmi_class_present()
    } else {
        false
    };
    let acer_hardware = identity
        .system_vendor
        .as_deref()
        .map(|vendor| vendor.to_ascii_lowercase().contains("acer"))
        .unwrap_or(false);
    let model_lower = identity
        .system_model
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let anv15_51 = model_lower.contains("anv15-51");
    let acer = AcerControlReadiness {
        system_vendor: identity.system_vendor,
        system_model: identity.system_model,
        bios_version: identity.bios_version,
        acer_hardware,
        anv15_51,
        acer_gaming_wmi_present: acer_wmi_present,
        detail: if !acer_hardware {
            "This does not look like Acer hardware from registry identity.".into()
        } else if !wmi.service.running {
            "Acer WMI class probe skipped because Windows Management Instrumentation is not running.".into()
        } else if acer_wmi_present {
            "AcerGamingFunction WMI class is visible.".into()
        } else {
            "AcerGamingFunction WMI class is not visible in ROOT\\WMI. Acer fan and firmware power controls remain unavailable.".into()
        },
    };

    let ready = service.running && wmi.service.running && acer.acer_gaming_wmi_present;
    let mut blockers = Vec::new();
    if !service.installed {
        blockers.push("AeroForgeService is not installed");
    } else if !service.running {
        blockers.push("AeroForgeService is not running");
    }
    if wmi.disabled {
        blockers.push("Windows Management Instrumentation is disabled");
    } else if !wmi.service.running {
        blockers.push("Windows Management Instrumentation is not running");
    }
    if service.running && wmi.service.running && !acer.acer_gaming_wmi_present {
        blockers.push("AcerGamingFunction WMI is not visible");
    }

    HardwareReadinessSnapshot {
        service,
        wmi,
        acer,
        ready,
        repair_available: repair_script_path().is_some(),
        detail: if ready {
            "AeroForge service, WMI, and Acer control paths are ready.".into()
        } else {
            format!("Hardware control blocked: {}.", blockers.join("; "))
        },
    }
}

pub fn launch_repair() -> RepairLaunchResult {
    let Some(script_path) = repair_script_path() else {
        return RepairLaunchResult {
            launched: false,
            elevated: false,
            detail: format!(
                "Could not find {INSTALLER_SCRIPT}. Reinstall AeroForge with the setup installer."
            ),
            log_path: Some(INSTALLER_LOG.into()),
        };
    };

    let Some(service_source) = service_source_path(&script_path) else {
        return RepairLaunchResult {
            launched: false,
            elevated: false,
            detail: format!(
                "Could not find {SERVICE_EXE} beside the repair script. Reinstall AeroForge with the setup installer."
            ),
            log_path: Some(INSTALLER_LOG.into()),
        };
    };

    match shell_execute_elevated_powershell(&script_path, &service_source) {
        Ok(()) => RepairLaunchResult {
            launched: true,
            elevated: true,
            detail: "Repair launched. Approve the Windows administrator prompt; AeroForge will enable/start WMI, reinstall the service, then the app can refresh hardware readiness.".into(),
            log_path: Some(INSTALLER_LOG.into()),
        },
        Err(error) => RepairLaunchResult {
            launched: false,
            elevated: true,
            detail: format!("Could not launch elevated repair: {error}"),
            log_path: Some(INSTALLER_LOG.into()),
        },
    }
}

pub fn start_installed_service() -> ServiceStartResult {
    let service = read_service_readiness(AEROFORGE_SERVICE);
    if !service.installed {
        return ServiceStartResult {
            started: false,
            elevated: false,
            detail: format!("{AEROFORGE_SERVICE} is not installed. Use Repair Service And WMI."),
        };
    }
    if service.running {
        return ServiceStartResult {
            started: true,
            elevated: false,
            detail: format!("{AEROFORGE_SERVICE} is already running."),
        };
    }

    match run_sc(&["start", AEROFORGE_SERVICE]) {
        Ok(output) if service_start_succeeded(&output) => {
            if wait_for_service_running(std::time::Duration::from_secs(8)) {
                ServiceStartResult {
                    started: true,
                    elevated: false,
                    detail: format!("{AEROFORGE_SERVICE} started successfully."),
                }
            } else {
                ServiceStartResult {
                    started: false,
                    elevated: false,
                    detail: format!(
                        "{AEROFORGE_SERVICE} start was accepted, but the service did not report running yet."
                    ),
                }
            }
        }
        Ok(output) if service_already_running(&output) => ServiceStartResult {
            started: true,
            elevated: false,
            detail: format!("{AEROFORGE_SERVICE} is already running."),
        },
        Ok(output) if service_start_needs_elevation(&output) => {
            match shell_execute_elevated_service_start() {
                Ok(()) => ServiceStartResult {
                    started: false,
                    elevated: true,
                    detail: format!(
                        "Service start needs administrator permission. Approve the Windows prompt to start {AEROFORGE_SERVICE}."
                    ),
                },
                Err(error) => ServiceStartResult {
                    started: false,
                    elevated: true,
                    detail: format!("Could not launch elevated service start: {error}"),
                },
            }
        }
        Ok(output) => ServiceStartResult {
            started: false,
            elevated: false,
            detail: format!("Could not start {AEROFORGE_SERVICE}: {}", output.trim()),
        },
        Err(error) => ServiceStartResult {
            started: false,
            elevated: false,
            detail: format!("Could not start {AEROFORGE_SERVICE}: {error}"),
        },
    }
}

pub fn request_service_heal() -> ServiceHealResult {
    let service = read_service_readiness(AEROFORGE_SERVICE);
    if !service.installed {
        return ServiceHealResult {
            requested: false,
            task_available: healer_task_available(),
            service_running: false,
            detail: format!(
                "{AEROFORGE_SERVICE} is not installed. Run the repair installer once to create the service and background healer."
            ),
        };
    }
    if service.running {
        return ServiceHealResult {
            requested: false,
            task_available: healer_task_available(),
            service_running: true,
            detail: format!("{AEROFORGE_SERVICE} is already running."),
        };
    }

    if !healer_task_available() {
        return ServiceHealResult {
            requested: false,
            task_available: false,
            service_running: false,
            detail: format!(
                "The {HEALER_TASK_NAME} scheduled task is missing. Run Repair Service And WMI once to install the background healer."
            ),
        };
    }

    match run_schtasks(&["/Run", "/TN", HEALER_TASK_NAME]) {
        Ok(output) if task_run_succeeded(&output) => {
            let running = wait_for_service_running(std::time::Duration::from_secs(12));
            ServiceHealResult {
                requested: true,
                task_available: true,
                service_running: running,
                detail: if running {
                    format!("{AEROFORGE_SERVICE} was started by the background healer.")
                } else {
                    format!(
                        "The background healer was requested, but {AEROFORGE_SERVICE} has not reported running yet."
                    )
                },
            }
        }
        Ok(output) => ServiceHealResult {
            requested: false,
            task_available: true,
            service_running: false,
            detail: format!("Could not run {HEALER_TASK_NAME}: {}", output.trim()),
        },
        Err(error) => ServiceHealResult {
            requested: false,
            task_available: true,
            service_running: false,
            detail: format!("Could not run {HEALER_TASK_NAME}: {error}"),
        },
    }
}

fn read_service_readiness(name: &str) -> WindowsServiceReadiness {
    let query = run_sc(&["query", name]);
    let installed = query
        .as_ref()
        .map(|text| !service_missing(text))
        .unwrap_or(false);
    if !installed {
        return WindowsServiceReadiness {
            name: name.into(),
            installed: false,
            running: false,
            start_type: None,
            detail: format!("{name} is not installed."),
        };
    }

    let running = query
        .as_ref()
        .ok()
        .and_then(|text| parse_running_state(text))
        .unwrap_or(false);
    let start_type = run_sc(&["qc", name])
        .ok()
        .and_then(|text| parse_start_type(&text));

    WindowsServiceReadiness {
        name: name.into(),
        installed,
        running,
        start_type: start_type.clone(),
        detail: format!(
            "{name} is {} with startup {}.",
            if running { "running" } else { "not running" },
            start_type.unwrap_or_else(|| "unknown".into())
        ),
    }
}

fn run_sc(args: &[&str]) -> Result<String, String> {
    run_tool("sc.exe", args)
}

fn run_schtasks(args: &[&str]) -> Result<String, String> {
    run_tool("schtasks.exe", args)
}

fn run_tool(program: &str, args: &[&str]) -> Result<String, String> {
    let output = hidden_command(program)
        .args(args)
        .output()
        .map_err(|error| error.to_string())?;
    Ok(String::from_utf8_lossy(&output.stdout).to_string()
        + &String::from_utf8_lossy(&output.stderr))
}

fn healer_task_available() -> bool {
    run_schtasks(&["/Query", "/TN", HEALER_TASK_NAME])
        .map(|text| !text.to_ascii_lowercase().contains("error:"))
        .unwrap_or(false)
}

fn task_run_succeeded(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("success") || lower.contains("has been attempted")
}

fn service_start_succeeded(text: &str) -> bool {
    let upper = text.to_ascii_uppercase();
    upper.contains("START_PENDING") || upper.contains("RUNNING")
}

fn service_already_running(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    text.contains("1056") || lower.contains("already been started")
}

fn service_start_needs_elevation(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    text.contains("5") && (lower.contains("access is denied") || lower.contains("access denied"))
}

fn wait_for_service_running(timeout: std::time::Duration) -> bool {
    let started = std::time::Instant::now();
    while started.elapsed() <= timeout {
        if read_service_readiness(AEROFORGE_SERVICE).running {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    false
}

fn service_missing(text: &str) -> bool {
    text.contains("1060") || text.to_ascii_lowercase().contains("does not exist")
}

fn parse_running_state(text: &str) -> Option<bool> {
    for line in text.lines() {
        if line.trim_start().starts_with("STATE") {
            let upper = line.to_ascii_uppercase();
            if upper.contains("RUNNING") {
                return Some(true);
            }
            if upper.contains("STOPPED")
                || upper.contains("START_PENDING")
                || upper.contains("STOP_PENDING")
            {
                return Some(false);
            }
        }
    }
    None
}

fn parse_start_type(text: &str) -> Option<String> {
    for line in text.lines() {
        if line.trim_start().starts_with("START_TYPE") {
            let normalized = line
                .split(':')
                .nth(1)?
                .split_whitespace()
                .skip(1)
                .collect::<Vec<_>>()
                .join(" ");
            if !normalized.is_empty() {
                return Some(normalized);
            }
        }
    }
    None
}

fn is_disabled_start_type(start_type: &str) -> bool {
    start_type.to_ascii_uppercase().contains("DISABLED")
}

#[derive(Clone, Default)]
struct RegistryHardwareIdentity {
    system_vendor: Option<String>,
    system_model: Option<String>,
    bios_version: Option<String>,
}

fn read_registry_hardware_identity() -> RegistryHardwareIdentity {
    let raw = hidden_command("reg.exe")
        .args(["query", r"HKLM\HARDWARE\DESCRIPTION\System\BIOS"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).to_string())
        .unwrap_or_default();

    RegistryHardwareIdentity {
        system_vendor: parse_reg_value(&raw, "SystemManufacturer"),
        system_model: parse_reg_value(&raw, "SystemProductName"),
        bios_version: parse_reg_value(&raw, "BIOSVersion"),
    }
}

fn parse_reg_value(raw: &str, name: &str) -> Option<String> {
    raw.lines().find_map(|line| {
        let trimmed = line.trim();
        if !trimmed.starts_with(name) {
            return None;
        }
        let mut parts = trimmed.split_whitespace();
        parts.next()?;
        parts.next()?;
        let value = parts.collect::<Vec<_>>().join(" ");
        (!value.is_empty()).then_some(value)
    })
}

/// The class is registered by the Acer WMI driver and does not go away once
/// seen, so a positive probe is remembered: this runs on every poll tick and a
/// PowerShell launch each time was the app's single biggest steady-state cost.
fn acer_gaming_wmi_class_present() -> bool {
    static PRESENT: AtomicBool = AtomicBool::new(false);
    if PRESENT.load(Ordering::Relaxed) {
        return true;
    }
    let script = "Get-CimClass -Namespace root\\wmi -ClassName AcerGamingFunction -ErrorAction Stop | Out-Null";
    let present = hidden_powershell()
        .args(["-Command", script])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    PRESENT.fetch_or(present, Ordering::Relaxed);
    present
}

fn repair_script_path() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(exe) = env::current_exe() {
        if let Some(parent) = exe.parent() {
            candidates.push(parent.join(INSTALLER_SCRIPT));
            candidates.push(parent.join("resources").join(INSTALLER_SCRIPT));
        }
    }
    if let Ok(cwd) = env::current_dir() {
        candidates.push(
            cwd.join("src-tauri")
                .join("resources")
                .join(INSTALLER_SCRIPT),
        );
        candidates.push(cwd.join("resources").join(INSTALLER_SCRIPT));
        candidates.push(cwd.join(INSTALLER_SCRIPT));
    }

    candidates.into_iter().find(|path| path.is_file())
}

fn service_source_path(script_path: &Path) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(parent) = script_path.parent() {
        candidates.push(parent.join(SERVICE_EXE));
    }
    if let Ok(exe) = env::current_exe() {
        if let Some(parent) = exe.parent() {
            candidates.push(parent.join(SERVICE_EXE));
        }
    }
    if let Ok(cwd) = env::current_dir() {
        candidates.push(
            cwd.join("aeroforge-service")
                .join("target")
                .join("release")
                .join(SERVICE_EXE),
        );
    }

    candidates.into_iter().find(|path| path.is_file())
}

#[cfg(windows)]
fn shell_execute_elevated_powershell(
    script_path: &Path,
    service_source: &Path,
) -> Result<(), String> {
    run_elevated_powershell(&format!(
        "-NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File {} -RepairPrerequisites -ServiceSource {}",
        quote_arg(script_path),
        quote_arg(service_source)
    ))
}

#[cfg(not(windows))]
fn shell_execute_elevated_powershell(
    _script_path: &Path,
    _service_source: &Path,
) -> Result<(), String> {
    Err("Hardware repair is only available on Windows.".into())
}

#[cfg(windows)]
fn shell_execute_elevated_service_start() -> Result<(), String> {
    run_elevated_powershell(
        "-NoProfile -ExecutionPolicy Bypass -Command \"Start-Service -Name AeroForgeService\"",
    )
}

/// Elevate Windows PowerShell by absolute path: a bare "powershell.exe" would let
/// ShellExecute pick one up from the (user-writable, in portable installs) app
/// directory and hand it the UAC-approved token.
#[cfg(windows)]
fn run_elevated_powershell(parameters: &str) -> Result<(), String> {
    use std::{ffi::OsStr, os::windows::ffi::OsStrExt};
    use windows_sys::Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_HIDE};

    fn wide(value: &OsStr) -> Vec<u16> {
        value.encode_wide().chain(Some(0)).collect()
    }

    let operation = wide(OsStr::new("runas"));
    let executable = wide(super::process::system32_path("powershell").as_os_str());
    let parameters = wide(OsStr::new(parameters));

    let result = super::process::with_sta_com(|| unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            operation.as_ptr(),
            executable.as_ptr(),
            parameters.as_ptr(),
            std::ptr::null(),
            SW_HIDE,
        )
    });

    let result_code = result as isize;
    if result_code > 32 {
        Ok(())
    } else {
        Err(format!("ShellExecuteW returned {result_code}."))
    }
}

#[cfg(not(windows))]
fn shell_execute_elevated_service_start() -> Result<(), String> {
    Err("Service start is only available on Windows.".into())
}

fn quote_arg(path: &Path) -> String {
    format!("\"{}\"", path.display().to_string().replace('"', "\\\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_running_service_state() {
        let sample = r#"
SERVICE_NAME: Winmgmt
        STATE              : 4  RUNNING
"#;
        assert_eq!(parse_running_state(sample), Some(true));
    }

    #[test]
    fn parses_disabled_start_type() {
        let sample = r#"
SERVICE_NAME: Winmgmt
        START_TYPE         : 4   DISABLED
"#;
        assert_eq!(parse_start_type(sample).as_deref(), Some("DISABLED"));
        assert!(is_disabled_start_type("DISABLED"));
    }

    #[test]
    fn detects_missing_service() {
        assert!(service_missing(
            "[SC] OpenService FAILED 1060:\nThe specified service does not exist as an installed service."
        ));
    }

    #[test]
    fn classifies_service_start_results() {
        assert!(service_start_succeeded(
            "STATE              : 2  START_PENDING"
        ));
        assert!(service_already_running(
            "[SC] StartService FAILED 1056:\nAn instance of the service is already running."
        ));
        assert!(service_start_needs_elevation(
            "[SC] StartService FAILED 5:\nAccess is denied."
        ));
    }

    #[test]
    fn classifies_healer_task_run_results() {
        assert!(task_run_succeeded(
            "SUCCESS: Attempted to run the scheduled task \"AeroForge Service Healer\"."
        ));
        assert!(!task_run_succeeded(
            "ERROR: The system cannot find the file specified."
        ));
    }

    #[test]
    fn parses_registry_identity_values() {
        let sample = r#"
    SystemManufacturer    REG_SZ    Acer
    SystemProductName    REG_SZ    Nitro ANV15-51
    BIOSVersion    REG_SZ    V1.60
"#;
        assert_eq!(
            parse_reg_value(sample, "SystemManufacturer").as_deref(),
            Some("Acer")
        );
        assert_eq!(
            parse_reg_value(sample, "SystemProductName").as_deref(),
            Some("Nitro ANV15-51")
        );
        assert_eq!(
            parse_reg_value(sample, "BIOSVersion").as_deref(),
            Some("V1.60")
        );
    }
}
