use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::RwLock,
    time::{SystemTime, UNIX_EPOCH},
};

use super::models::{
    ApplyState, BackendContract, BootArtId, CapabilitySnapshot, CommandDescriptor, ControlSnapshot,
    CustomPowerBaseId, FanCurvePoint, FanCurveSet, FanProfileId, FeatureSupport, GpuTuningState,
    OcPreset, PersistenceStatus, PersonalSettings, PowerProfileId, ProcessorStateSettings,
    ShellStatus, TelemetrySnapshot, UpdateChannelId, UpdateStatus,
};
use super::updater::UpdaterStore;

pub struct BackendState {
    contract: BackendContract,
    capabilities: CapabilitySnapshot,
    default_controls: ControlSnapshot,
    controls: RwLock<ControlSnapshot>,
    telemetry: RwLock<TelemetrySnapshot>,
    config_file: PathBuf,
    updater: UpdaterStore,
    initialized_from_disk: bool,
}

impl BackendState {
    pub fn load(config_root: PathBuf) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        fs::create_dir_all(&config_root)?;
        let config_file = config_root.join("control-state.json");

        let contract = build_contract();
        let capabilities = build_capabilities();
        let default_controls = build_default_controls();
        let telemetry = build_default_telemetry();
        let updater = UpdaterStore::load(&config_root)?;

        let (controls, initialized_from_disk) = load_controls(&config_file, &default_controls)?;

        let state = Self {
            contract,
            capabilities,
            default_controls,
            controls: RwLock::new(controls),
            telemetry: RwLock::new(telemetry),
            config_file,
            updater,
            initialized_from_disk,
        };

        if !state.initialized_from_disk {
            state.persist_controls()?;
        }

        Ok(state)
    }

    pub fn contract(&self) -> BackendContract {
        self.contract.clone()
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

    pub fn save_controls(
        &self,
        snapshot: ControlSnapshot,
    ) -> Result<ControlSnapshot, Box<dyn std::error::Error + Send + Sync>> {
        {
            let mut controls = self
                .controls
                .write()
                .expect("backend controls lock poisoned");
            *controls = snapshot.clone();
        }

        self.persist_controls()?;
        Ok(snapshot)
    }

    pub fn reset_controls(
        &self,
    ) -> Result<ControlSnapshot, Box<dyn std::error::Error + Send + Sync>> {
        self.save_controls(self.default_controls.clone())
    }

    fn persist_controls(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let serialized = serde_json::to_string_pretty(&self.controls())?;
        fs::write(&self.config_file, serialized)?;
        Ok(())
    }
}

fn load_controls(
    config_file: &Path,
    defaults: &ControlSnapshot,
) -> Result<(ControlSnapshot, bool), Box<dyn std::error::Error + Send + Sync>> {
    if !config_file.exists() {
        return Ok((defaults.clone(), false));
    }

    let raw = fs::read_to_string(config_file)?;
    if raw.trim().is_empty() {
        quarantine_invalid_state_file(config_file, "empty")?;
        return Ok((defaults.clone(), false));
    }

    match serde_json::from_str::<ControlSnapshot>(&raw) {
        Ok(parsed) => Ok((parsed, true)),
        Err(_) => {
            quarantine_invalid_state_file(config_file, "invalid")?;
            Ok((defaults.clone(), false))
        }
    }
}

fn quarantine_invalid_state_file(path: &Path, reason: &str) -> Result<(), io::Error> {
    if !path.exists() {
        return Ok(());
    }

    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("control-state.json");
    let backup_name = format!("{file_name}.{reason}.{stamp}.bak");
    let backup_path = path.with_file_name(backup_name);
    fs::rename(path, backup_path)
}

fn build_contract() -> BackendContract {
    BackendContract {
        schema_version: "0.2.0".into(),
        commands: vec![
            CommandDescriptor {
                command: "runtime_shell".into(),
                stage: "implemented".into(),
                purpose: "Return shell and backend version information.".into(),
            },
            CommandDescriptor {
                command: "get_backend_contract".into(),
                stage: "implemented".into(),
                purpose: "Return the command schema exposed by the desktop backend.".into(),
            },
            CommandDescriptor {
                command: "get_service_status".into(),
                stage: "implemented".into(),
                purpose: "Return whether the AeroForge Windows service is reachable over the named pipe.".into(),
            },
            CommandDescriptor {
                command: "get_capability_snapshot".into(),
                stage: "implemented".into(),
                purpose: "Describe supported hardware control paths, preferring the AeroForge service when reachable.".into(),
            },
            CommandDescriptor {
                command: "get_control_snapshot".into(),
                stage: "implemented".into(),
                purpose: "Return the staged control state mirrored by the frontend today.".into(),
            },
            CommandDescriptor {
                command: "get_telemetry_snapshot".into(),
                stage: "implemented".into(),
                purpose: "Return the current telemetry snapshot, preferring the AeroForge service when reachable.".into(),
            },
            CommandDescriptor {
                command: "get_backend_bootstrap".into(),
                stage: "implemented".into(),
                purpose: "Return a single payload with shell, contract, capabilities, controls, and telemetry.".into(),
            },
            CommandDescriptor {
                command: "get_persistence_status".into(),
                stage: "implemented".into(),
                purpose: "Return where AeroForge stores its app-owned control state.".into(),
            },
            CommandDescriptor {
                command: "get_update_status".into(),
                stage: "implemented".into(),
                purpose: "Return the cached GitHub updater state and any staged update asset details.".into(),
            },
            CommandDescriptor {
                command: "check_for_updates".into(),
                stage: "implemented".into(),
                purpose: "Query the published GitHub release feed for the latest build metadata.".into(),
            },
            CommandDescriptor {
                command: "stage_update_download".into(),
                stage: "implemented".into(),
                purpose: "Download the latest setup EXE or portable ZIP into AeroForge's local staging directory.".into(),
            },
            CommandDescriptor {
                command: "install_staged_update".into(),
                stage: "implemented".into(),
                purpose: "Launch the portable update handoff script for a previously staged ZIP build.".into(),
            },
            CommandDescriptor {
                command: "save_control_snapshot".into(),
                stage: "implemented".into(),
                purpose: "Persist AeroForge-owned control state to disk.".into(),
            },
            CommandDescriptor {
                command: "reset_control_snapshot".into(),
                stage: "implemented".into(),
                purpose: "Restore AeroForge-owned control state to backend defaults.".into(),
            },
            CommandDescriptor {
                command: "detect_capabilities".into(),
                stage: "planned".into(),
                purpose: "Probe real system support and privilege requirements.".into(),
            },
            CommandDescriptor {
                command: "apply_power_profile".into(),
                stage: "implemented".into(),
                purpose: "Apply a power profile through the AeroForge service and persist the staged processor policy.".into(),
            },
            CommandDescriptor {
                command: "apply_fan_profile".into(),
                stage: "implemented".into(),
                purpose: "Apply Auto, Max, or Custom cooling behavior through the AeroForge service.".into(),
            },
            CommandDescriptor {
                command: "apply_custom_fan_curves".into(),
                stage: "implemented".into(),
                purpose: "Validate CPU and GPU fan curves and write current-temperature targets through the AeroForge service.".into(),
            },
            CommandDescriptor {
                command: "apply_smart_charging".into(),
                stage: "implemented".into(),
                purpose: "Apply Acer Care Center BatteryHealthy charging mode and persist the staged smart-charge state.".into(),
            },
            CommandDescriptor {
                command: "set_charge_behavior".into(),
                stage: "planned".into(),
                purpose: "Control smart charging, USB power, and related battery behaviors.".into(),
            },
            CommandDescriptor {
                command: "apply_blue_light_filter".into(),
                stage: "implemented".into(),
                purpose: "Apply the Acer-style blue light gamma ramp and persist the staged eye-care state.".into(),
            },
            CommandDescriptor {
                command: "apply_gpu_tuning".into(),
                stage: "implemented".into(),
                purpose: "Apply GPU clock tuning through the AeroForge service and persist the staged tuning state.".into(),
            },
            CommandDescriptor {
                command: "apply_boot_logo".into(),
                stage: "implemented".into(),
                purpose: "Convert, stage, and apply a boot-logo image through the service.".into(),
            },
        ],
    }
}

fn build_capabilities() -> CapabilitySnapshot {
    CapabilitySnapshot {
        power_profiles: FeatureSupport {
            available: true,
            writable: true,
            requires_elevation: true,
        },
        fan_profiles: FeatureSupport {
            available: true,
            writable: true,
            requires_elevation: true,
        },
        fan_curves: FeatureSupport {
            available: true,
            writable: true,
            requires_elevation: true,
        },
        smart_charging: FeatureSupport {
            available: true,
            writable: true,
            requires_elevation: false,
        },
        usb_power: FeatureSupport {
            available: true,
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
            "Smart charging now uses Acer Care Center's local BatteryHealthy websocket path on port 4343. BatteryHealthy 0 keeps the 80% optimized cap, while BatteryHealthy 1 restores full charging.".into(),
            "Power-profile apply now uses direct AcerGamingFunction operating-mode writes for supported modes, then applies the staged Windows processor policy.".into(),
            "GPU tuning apply now flows through the AeroForge service and currently writes editable NVAPI P0 clock offsets while staging unsupported voltage and limit fields.".into(),
            "Fan profile and curve apply now flow through the AeroForge service using direct ROOT\\WMI AcerGamingFunction ACPI calls, with RPM movement verified through telemetry.".into(),
            "Boot-logo upload now converts user images to firmware-safe JPEG and asks the AeroForge service to apply CUSTOM_BOOT_LOGO through the clean-room AcerAgentService packet path.".into(),
        ],
    }
}

fn build_default_controls() -> ControlSnapshot {
    let default_gpu_tuning = GpuTuningState {
        core_clock_mhz: 165,
        memory_clock_mhz: 420,
        voltage_offset_mv: -35,
        power_limit_percent: 114,
        temp_limit_c: 83,
    };

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
                name: "Silent UV".into(),
                strap: "Low-noise undervolt".into(),
                settings: GpuTuningState {
                    core_clock_mhz: 90,
                    memory_clock_mhz: 180,
                    voltage_offset_mv: -60,
                    power_limit_percent: 92,
                    temp_limit_c: 78,
                },
                is_custom: false,
            },
            OcPreset {
                id: "daily".into(),
                label: "P2".into(),
                name: "Forge Daily".into(),
                strap: "Balanced everyday tune".into(),
                settings: default_gpu_tuning.clone(),
                is_custom: false,
            },
            OcPreset {
                id: "creator".into(),
                label: "P3".into(),
                name: "Creator Boost".into(),
                strap: "Long-session render preset".into(),
                settings: GpuTuningState {
                    core_clock_mhz: 185,
                    memory_clock_mhz: 560,
                    voltage_offset_mv: -10,
                    power_limit_percent: 118,
                    temp_limit_c: 84,
                },
                is_custom: false,
            },
            OcPreset {
                id: "arena".into(),
                label: "P4".into(),
                name: "Arena Max".into(),
                strap: "Aggressive gaming tune".into(),
                settings: GpuTuningState {
                    core_clock_mhz: 220,
                    memory_clock_mhz: 840,
                    voltage_offset_mv: 25,
                    power_limit_percent: 122,
                    temp_limit_c: 86,
                },
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
        fan_sync_lock_enabled: false,
        personal_settings: PersonalSettings {
            smart_charging_enabled: true,
            usb_power_enabled: true,
            blue_light_filter_enabled: false,
            selected_boot_art: BootArtId::Ember,
            custom_boot_filename: "custom-boot.png".into(),
            update_channel: UpdateChannelId::Stable,
            check_for_updates_on_launch: true,
        },
    }
}

fn build_default_telemetry() -> TelemetrySnapshot {
    TelemetrySnapshot {
        cpu_temp_c: 0,
        cpu_temp_average_c: None,
        cpu_temp_lowest_core_c: None,
        cpu_temp_highest_core_c: None,
        gpu_temp_c: 0,
        system_temp_c: 0,
        cpu_usage_percent: 0,
        gpu_usage_percent: 0,
        gpu_memory_usage_percent: None,
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
