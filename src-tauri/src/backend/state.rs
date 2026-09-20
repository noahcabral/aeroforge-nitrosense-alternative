use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        RwLock,
    },
};

use super::models::{
    ApplyState, BootArtId, CapabilitySnapshot, ControlSnapshot, CustomPowerBaseId, FanCurvePoint,
    FanCurveSet, FanProfileId, FeatureSupport, GpuTuningState, OcPreset, PersistenceStatus,
    PersonalSettings, PowerProfileId, ProcessorStateSettings, ShellStatus, TelemetrySnapshot,
    UpdateChannelId, UpdateStatus,
};
use super::now_unix;
use super::process::hidden_command;
use super::updater::UpdaterStore;
use super::DynError;

const ANV15_51_GPU_MEMORY_OFFSETS: [(&str, i16); 3] = [
    ("PCI\\VEN_10DE&DEV_25AC&SUBSYS_171E1025", 0),
    ("PCI\\VEN_10DE&DEV_25AD&SUBSYS_171E1025", 200),
    ("PCI\\VEN_10DE&DEV_28A1&SUBSYS_171E1025", 200),
];

pub struct BackendState {
    capabilities: CapabilitySnapshot,
    controls: RwLock<ControlSnapshot>,
    telemetry: RwLock<TelemetrySnapshot>,
    config_file: PathBuf,
    updater: UpdaterStore,
    initialized_from_disk: bool,
}

impl BackendState {
    pub fn load(config_root: PathBuf) -> Result<Self, DynError> {
        fs::create_dir_all(&config_root)?;
        let config_file = config_root.join("control-state.json");

        let capabilities = build_capabilities();
        let default_controls = build_default_controls();
        let telemetry = build_default_telemetry();
        let updater = UpdaterStore::load(&config_root)?;

        let (controls, initialized_from_disk) = load_controls(&config_file, &default_controls)?;
        if !initialized_from_disk {
            persist_controls(&config_file, &controls)?;
        }

        Ok(Self {
            capabilities,
            controls: RwLock::new(controls),
            telemetry: RwLock::new(telemetry),
            config_file,
            updater,
            initialized_from_disk,
        })
    }

    pub fn capabilities(&self) -> CapabilitySnapshot {
        self.capabilities.clone()
    }

    pub fn controls(&self) -> ControlSnapshot {
        self.controls
            .read()
            .expect("backend controls lock poisoned")
            .clone()
    }

    pub fn telemetry(&self) -> TelemetrySnapshot {
        self.telemetry
            .read()
            .expect("backend telemetry lock poisoned")
            .clone()
    }

    pub fn persistence_status(&self) -> PersistenceStatus {
        PersistenceStatus {
            config_file: self.config_file.display().to_string(),
            initialized_from_disk: self.initialized_from_disk,
        }
    }

    pub fn config_root(&self) -> PathBuf {
        self.config_file
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."))
    }

    pub fn updater(&self) -> &UpdaterStore {
        &self.updater
    }

    pub fn update_status(&self) -> UpdateStatus {
        self.updater.status()
    }

    pub fn save_controls(&self, snapshot: ControlSnapshot) -> Result<ControlSnapshot, DynError> {
        // The lock stays held across the file write so concurrent saves reach
        // disk in the same order they reached memory.
        let mut controls = self
            .controls
            .write()
            .expect("backend controls lock poisoned");
        *controls = snapshot;
        persist_controls(&self.config_file, &controls)?;
        Ok(controls.clone())
    }
}

fn persist_controls(config_file: &Path, controls: &ControlSnapshot) -> Result<(), DynError> {
    write_text_atomically(config_file, &serde_json::to_string_pretty(controls)?)
}

/// Write via a sibling temp file plus rename so a crash or power loss mid-write
/// can never leave a truncated file behind (rename replaces in place on NTFS).
/// Each call gets its own temp file so two writers of the same path never
/// share one.
pub(crate) fn write_text_atomically(path: &Path, content: &str) -> Result<(), DynError> {
    let parent = path.parent().ok_or("config path has no parent directory")?;
    fs::create_dir_all(parent)?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("config");
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let temp_path = parent.join(format!(
        ".{file_name}.{}.{}.tmp",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| {
        let mut file = fs::File::create(&temp_path)?;
        io::Write::write_all(&mut file, content.as_bytes())?;
        file.sync_all()?;
        fs::rename(&temp_path, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    Ok(result?)
}

fn load_controls(
    config_file: &Path,
    defaults: &ControlSnapshot,
) -> Result<(ControlSnapshot, bool), DynError> {
    if !config_file.exists() {
        return Ok((defaults.clone(), false));
    }

    let raw = fs::read_to_string(config_file)?;
    if raw.trim().is_empty() {
        quarantine_invalid_state_file(config_file, "empty")?;
        return Ok((defaults.clone(), false));
    }

    match serde_json::from_str::<ControlSnapshot>(&raw) {
        Ok(parsed) => Ok((normalize_loaded_controls(parsed, defaults), true)),
        Err(_) => {
            quarantine_invalid_state_file(config_file, "invalid")?;
            Ok((defaults.clone(), false))
        }
    }
}

fn normalize_loaded_controls(
    mut controls: ControlSnapshot,
    defaults: &ControlSnapshot,
) -> ControlSnapshot {
    let default_builtin_presets: Vec<OcPreset> = defaults
        .oc_presets
        .iter()
        .filter(|preset| !preset.is_custom)
        .cloned()
        .collect();
    let default_builtin_ids: Vec<&str> = default_builtin_presets
        .iter()
        .map(|preset| preset.id.as_str())
        .collect();
    let saved_custom_preset = controls
        .oc_presets
        .iter()
        .find(|preset| preset.is_custom)
        .cloned()
        .or_else(|| {
            defaults
                .oc_presets
                .iter()
                .find(|preset| preset.is_custom)
                .cloned()
        });
    let saved_extra_presets = controls
        .oc_presets
        .iter()
        .filter(|preset| !preset.is_custom && !default_builtin_ids.contains(&preset.id.as_str()))
        .cloned()
        .collect::<Vec<_>>();

    controls.oc_presets = default_builtin_presets;
    controls.oc_presets.extend(saved_extra_presets);
    if let Some(custom_preset) = saved_custom_preset {
        controls.oc_presets.push(custom_preset);
    }

    if !controls
        .oc_presets
        .iter()
        .any(|preset| preset.id == controls.active_oc_slot)
    {
        controls.active_oc_slot = defaults.active_oc_slot.clone();
    }

    if let Some(active_builtin) = controls
        .oc_presets
        .iter()
        .find(|preset| preset.id == controls.active_oc_slot && !preset.is_custom)
    {
        controls.gpu_tuning = active_builtin.settings.clone();
    }

    controls
}

fn quarantine_invalid_state_file(path: &Path, reason: &str) -> Result<(), io::Error> {
    if !path.exists() {
        return Ok(());
    }

    let stamp = now_unix();
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("control-state.json");
    let backup_name = format!("{file_name}.{reason}.{stamp}.bak");
    let backup_path = path.with_file_name(backup_name);
    fs::rename(path, backup_path)
}

fn build_capabilities() -> CapabilitySnapshot {
    CapabilitySnapshot {
        power_profiles: FeatureSupport {
            available: true,
            writable: false,
            requires_elevation: true,
        },
        fan_profiles: FeatureSupport {
            available: true,
            writable: false,
            requires_elevation: true,
        },
        fan_curves: FeatureSupport {
            available: true,
            writable: false,
            requires_elevation: true,
        },
        smart_charging: FeatureSupport {
            available: true,
            writable: true,
            requires_elevation: false,
        },
        usb_power: FeatureSupport {
            available: false,
            writable: false,
            requires_elevation: true,
        },
        blue_light_filter: FeatureSupport {
            available: true,
            writable: true,
            requires_elevation: false,
        },
        gpu_tuning: FeatureSupport {
            available: true,
            writable: true,
            requires_elevation: true,
        },
        boot_logo: FeatureSupport {
            available: true,
            writable: true,
            requires_elevation: true,
        },
        notes: vec![
            "When the AeroForge service named pipe is unavailable, the backend prefers cached service state files before falling back to typed zero/default telemetry.".into(),
            "AeroForge-owned control state is now persisted to disk; named-pipe service IPC is now in place for read-only snapshots.".into(),
            "GitHub updater state is stored separately from control-state.json, and release checks now use the public GitHub release feed.".into(),
            "Blue light filter apply now uses a clean-room gamma ramp implementation matched to Acer Quick Access GainID 0-4 behavior and also updates the Quick Access settings file.".into(),
            "Smart charging now prefers the AeroForge service named-pipe path so BatteryControl health-mode writes run with service elevation, with desktop BatteryControl and Acer Care Center fallback only when the service is unavailable.".into(),
            "Power-off USB charging is still staged out of the UI because AeroForge does not yet have a verified direct Windows control surface for that feature.".into(),
            "Auto 60 Hz on battery uses the Windows display-mode API from the user-session backend and stores the previous refresh rate for AC/disable restore.".into(),
            "Power-profile apply now prefers direct AcerGamingFunction platform-profile misc-setting writes, falls back to legacy profile writes when needed, then applies the staged Windows processor policy.".into(),
            "GPU tuning apply now flows through the AeroForge service and currently writes editable NVAPI P0 clock offsets while staging unsupported voltage and limit fields.".into(),
            "Fan profile and curve apply now flow through the AeroForge service using direct ROOT\\WMI AcerGamingFunction ACPI calls, with direct Acer sensor RPM readback preferred for telemetry.".into(),
            "Boot-logo apply is serviced by AeroForge directly through the EFI System Partition; the write path requires elevation and refuses ambiguous ESP targets.".into(),
        ],
        cache_updated_at_unix: None,
    }
}

fn build_default_controls() -> ControlSnapshot {
    let default_gpu_tuning = GpuTuningState {
        core_clock_mhz: 0,
        memory_clock_mhz: 0,
        voltage_offset_mv: 0,
        power_limit_percent: 100,
        temp_limit_c: 83,
    };
    let nitro_sense_extreme_gpu_tuning = nitrosense_extreme_gpu_tuning();

    ControlSnapshot {
        active_power_profile: PowerProfileId::Balanced,
        active_fan_profile: FanProfileId::Auto,
        custom_processor_state: ProcessorStateSettings {
            min_percent: 35,
            max_percent: 88,
        },
        custom_power_base: CustomPowerBaseId::Performance,
        gpu_tuning: default_gpu_tuning.clone(),
        oc_presets: vec![
            OcPreset {
                id: "silent-uv".into(),
                label: "P1".into(),
                name: "Acer Quiet".into(),
                strap: "NitroSense quiet GPU tune".into(),
                settings: default_gpu_tuning.clone(),
                is_custom: false,
            },
            OcPreset {
                id: "daily".into(),
                label: "P2".into(),
                name: "Acer Default".into(),
                strap: "NitroSense default GPU tune".into(),
                settings: default_gpu_tuning.clone(),
                is_custom: false,
            },
            OcPreset {
                id: "creator".into(),
                label: "P3".into(),
                name: "Acer Performance".into(),
                strap: "NitroSense performance offset".into(),
                settings: nitro_sense_extreme_gpu_tuning.clone(),
                is_custom: false,
            },
            OcPreset {
                id: "arena".into(),
                label: "P4".into(),
                name: "Acer Turbo".into(),
                strap: "NitroSense turbo offset".into(),
                settings: nitro_sense_extreme_gpu_tuning,
                is_custom: false,
            },
            OcPreset {
                id: "custom-user".into(),
                label: "P5".into(),
                name: "Custom Preset".into(),
                strap: "User-saved GPU tuning".into(),
                settings: default_gpu_tuning,
                is_custom: true,
            },
        ],
        active_oc_slot: "daily".into(),
        oc_apply_state: ApplyState::Live,
        oc_tuning_locked: false,
        fan_curves: FanCurveSet {
            cpu: default_curve_points(),
            gpu: default_curve_points(),
        },
        fan_presets: Vec::new(),
        power_presets: Vec::new(),
        quick_presets: Vec::new(),
        fan_sync_lock_enabled: false,
        personal_settings: PersonalSettings {
            smart_charging_enabled: true,
            usb_power_enabled: true,
            processor_state_control_enabled: true,
            nvidia_telemetry_enabled: false,
            keep_ui_prewarmed: false,
            blue_light_filter_enabled: false,
            auto_refresh_rate_on_battery_enabled: false,
            auto_refresh_rate_restore_hz: None,
            selected_boot_art: BootArtId::Ember,
            custom_boot_filename: "custom-boot.png".into(),
            update_channel: UpdateChannelId::Stable,
            check_for_updates_on_launch: true,
        },
    }
}

fn build_default_telemetry() -> TelemetrySnapshot {
    TelemetrySnapshot {
        cache_updated_at_unix: None,
        cpu_temp_c: 0,
        cpu_temp_average_c: None,
        cpu_temp_lowest_core_c: None,
        cpu_temp_highest_core_c: None,
        gpu_temp_c: 0,
        system_temp_c: 0,
        cpu_usage_percent: 0,
        gpu_usage_percent: 0,
        gpu_memory_usage_percent: None,
        gpu_power_draw_w: None,
        gpu_power_limit_w: None,
        gpu_power_default_limit_w: None,
        gpu_power_min_limit_w: None,
        gpu_power_max_limit_w: None,
        cpu_package_power_w: None,
        cpu_pl1_w: None,
        cpu_pl1_enabled: None,
        cpu_pl2_w: None,
        cpu_pl2_enabled: None,
        cpu_power_limit_locked: None,
        cpu_name: None,
        cpu_brand: None,
        gpu_name: None,
        gpu_brand: None,
        system_vendor: None,
        system_model: None,
        cpu_clock_mhz: 0,
        gpu_clock_mhz: 0,
        cpu_fan_rpm: 0,
        gpu_fan_rpm: 0,
        battery_percent: 0,
        battery_life_remaining_sec: None,
        ac_plugged_in: false,
    }
}

fn default_curve_points() -> Vec<FanCurvePoint> {
    vec![
        FanCurvePoint {
            temp_c: 30,
            speed_percent: 2,
        },
        FanCurvePoint {
            temp_c: 49,
            speed_percent: 2,
        },
        FanCurvePoint {
            temp_c: 65,
            speed_percent: 22,
        },
        FanCurvePoint {
            temp_c: 74,
            speed_percent: 64,
        },
        FanCurvePoint {
            temp_c: 80,
            speed_percent: 100,
        },
    ]
}

pub fn shell_status() -> ShellStatus {
    ShellStatus {
        shell: "Tauri desktop shell connected".into(),
        backend_version: env!("CARGO_PKG_VERSION").into(),
    }
}

fn nitrosense_extreme_gpu_tuning() -> GpuTuningState {
    GpuTuningState {
        core_clock_mhz: 100,
        memory_clock_mhz: nitrosense_anv15_51_memory_offset_mhz(),
        voltage_offset_mv: 0,
        power_limit_percent: 100,
        temp_limit_c: 83,
    }
}

fn nitrosense_anv15_51_memory_offset_mhz() -> i16 {
    if !detected_model_is_anv15_51() {
        return 200;
    }

    query_registry_tree(r"HKLM\SYSTEM\CurrentControlSet\Enum\PCI")
        .as_deref()
        .and_then(nitrosense_anv15_51_memory_offset_from_pci_text)
        .unwrap_or(200)
}

fn detected_model_is_anv15_51() -> bool {
    query_registry_value(
        r"HKLM\HARDWARE\DESCRIPTION\System\BIOS",
        "SystemProductName",
    )
    .map(|model| model.to_ascii_uppercase().contains("ANV15-51"))
    .unwrap_or(false)
}

fn query_registry_value(key: &str, value_name: &str) -> Option<String> {
    let text = reg_query(&[key, "/v", value_name])?;
    registry_value_from_query_output(&text, value_name)
}

fn query_registry_tree(key: &str) -> Option<String> {
    reg_query(&[key, "/s", "/f", "VEN_10DE"])
}

fn reg_query(args: &[&str]) -> Option<String> {
    let output = hidden_command("reg")
        .arg("query")
        .args(args)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

fn registry_value_from_query_output(raw: &str, value_name: &str) -> Option<String> {
    for line in raw.lines() {
        let mut parts = line.split_whitespace();
        let Some(name) = parts.next() else {
            continue;
        };
        if !name.eq_ignore_ascii_case(value_name) {
            continue;
        }
        let _registry_type = parts.next()?;
        let value = parts.collect::<Vec<_>>().join(" ");
        if !value.is_empty() {
            return Some(value);
        }
    }

    None
}

fn nitrosense_anv15_51_memory_offset_from_pci_text(raw: &str) -> Option<i16> {
    let uppercase = raw.to_ascii_uppercase();
    ANV15_51_GPU_MEMORY_OFFSETS
        .iter()
        .find(|(pci_id, _)| uppercase.contains(&pci_id.to_ascii_uppercase()))
        .map(|(_, memory_offset_mhz)| *memory_offset_mhz)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_replaces_in_place_and_leaves_no_temp_file() {
        let dir = std::env::temp_dir().join(format!("aeroforge-atomic-{}", std::process::id()));
        let path = dir.join("nested").join("control-state.json");

        write_text_atomically(&path, "first").unwrap();
        write_text_atomically(&path, "second").unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "second");
        let leftovers: Vec<_> = fs::read_dir(path.parent().unwrap())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "temp file left behind: {leftovers:?}");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn concurrent_atomic_writes_never_leave_a_torn_file() {
        let dir =
            std::env::temp_dir().join(format!("aeroforge-atomic-race-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("status.json");
        let bodies: Vec<String> = (0..8).map(|n| format!("{n}").repeat(64 * 1024)).collect();

        std::thread::scope(|scope| {
            for body in &bodies {
                scope.spawn(|| {
                    for _ in 0..20 {
                        write_text_atomically(&path, body).unwrap();
                    }
                });
            }
        });

        let written = fs::read_to_string(&path).unwrap();
        assert!(
            bodies.contains(&written),
            "file was torn: {} bytes",
            written.len()
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn default_controls_start_balanced_with_nitrosense_gpu_presets() {
        let controls = build_default_controls();

        assert!(matches!(
            controls.active_power_profile,
            PowerProfileId::Balanced
        ));

        let daily = controls
            .oc_presets
            .iter()
            .find(|preset| preset.id == "daily")
            .unwrap();
        assert_eq!(daily.name, "Acer Default");
        assert_eq!(daily.settings.core_clock_mhz, 0);
        assert_eq!(daily.settings.memory_clock_mhz, 0);

        let turbo = controls
            .oc_presets
            .iter()
            .find(|preset| preset.id == "arena")
            .unwrap();
        assert_eq!(turbo.name, "Acer Turbo");
        assert_eq!(turbo.settings.core_clock_mhz, 100);
        assert_eq!(turbo.settings.memory_clock_mhz, 200);
    }

    #[test]
    fn parses_anv15_51_nitrosense_gpu_memory_offsets_from_pci_text() {
        assert_eq!(
            nitrosense_anv15_51_memory_offset_from_pci_text(
                r"HKEY_LOCAL_MACHINE\SYSTEM\CurrentControlSet\Enum\PCI\VEN_10DE&DEV_25AC&SUBSYS_171E1025"
            ),
            Some(0)
        );
        assert_eq!(
            nitrosense_anv15_51_memory_offset_from_pci_text(
                r"HKEY_LOCAL_MACHINE\SYSTEM\CurrentControlSet\Enum\PCI\VEN_10DE&DEV_28A1&SUBSYS_171E1025"
            ),
            Some(200)
        );
        assert_eq!(
            nitrosense_anv15_51_memory_offset_from_pci_text(
                r"HKEY_LOCAL_MACHINE\SYSTEM\CurrentControlSet\Enum\PCI\VEN_10DE&DEV_28A1&SUBSYS_168A1025"
            ),
            None
        );
    }

    #[test]
    fn parses_registry_values_with_spaced_data() {
        let raw = r#"
HKEY_LOCAL_MACHINE\HARDWARE\DESCRIPTION\System\BIOS
    SystemProductName    REG_SZ    Nitro ANV15-51
"#;
        assert_eq!(
            registry_value_from_query_output(raw, "SystemProductName").as_deref(),
            Some("Nitro ANV15-51")
        );
    }

    #[test]
    fn normalizes_old_builtin_gpu_presets_without_touching_custom_slot() {
        let defaults = build_default_controls();
        let mut loaded = defaults.clone();
        loaded.active_oc_slot = "daily".into();
        loaded.gpu_tuning = GpuTuningState {
            core_clock_mhz: 165,
            memory_clock_mhz: 420,
            voltage_offset_mv: -35,
            power_limit_percent: 114,
            temp_limit_c: 83,
        };
        let custom = loaded
            .oc_presets
            .iter_mut()
            .find(|preset| preset.is_custom)
            .unwrap();
        custom.settings = GpuTuningState {
            core_clock_mhz: 42,
            memory_clock_mhz: 84,
            voltage_offset_mv: 0,
            power_limit_percent: 100,
            temp_limit_c: 80,
        };

        let normalized = normalize_loaded_controls(loaded, &defaults);
        let daily = normalized
            .oc_presets
            .iter()
            .find(|preset| preset.id == "daily")
            .unwrap();
        let custom = normalized
            .oc_presets
            .iter()
            .find(|preset| preset.is_custom)
            .unwrap();

        assert_eq!(daily.settings.core_clock_mhz, 0);
        assert_eq!(daily.settings.memory_clock_mhz, 0);
        assert_eq!(normalized.gpu_tuning.core_clock_mhz, 0);
        assert_eq!(normalized.gpu_tuning.memory_clock_mhz, 0);
        assert_eq!(custom.settings.core_clock_mhz, 42);
        assert_eq!(custom.settings.memory_clock_mhz, 84);
    }
}
