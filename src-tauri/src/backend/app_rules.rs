//! Per-app power and fan overrides. While a listed executable runs, its
//! profiles apply; once none run, the saved defaults from the control
//! snapshot come back. Rules live in their own file because the UI rebuilds
//! the control snapshot from scratch on every save.

use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    sync::RwLock,
    thread,
    time::Duration,
};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use super::{
    models::{ControlSnapshot, FanProfileId, PowerProfileId},
    service_pipe,
    state::{write_text_atomically, BackendState},
    tray, DynError,
};

const POLL_INTERVAL: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppRule {
    pub exe_name: String,
    pub power_profile: PowerProfileId,
    pub fan_profile: FanProfileId,
}

pub struct AppRuleStore {
    file: PathBuf,
    rules: RwLock<Vec<AppRule>>,
}

impl AppRuleStore {
    pub fn load(config_root: &Path) -> Self {
        let file = config_root.join("app-rules.json");
        let rules = fs::read_to_string(&file)
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default();
        Self {
            file,
            rules: RwLock::new(rules),
        }
    }

    pub fn rules(&self) -> Vec<AppRule> {
        self.rules.read().expect("app rules lock poisoned").clone()
    }

    pub fn save(&self, rules: Vec<AppRule>) -> Result<Vec<AppRule>, String> {
        let rules = normalize_rules(rules);
        let mut current = self.rules.write().expect("app rules lock poisoned");
        let json = serde_json::to_string_pretty(&rules).map_err(|error| error.to_string())?;
        write_text_atomically(&self.file, &json).map_err(|error| error.to_string())?;
        *current = rules;
        Ok(current.clone())
    }
}

/// Accepts "Game", "game.exe" or a full path; keeps the first rule per exe.
fn normalize_rules(rules: Vec<AppRule>) -> Vec<AppRule> {
    let mut seen = HashSet::new();
    rules
        .into_iter()
        .filter_map(|mut rule| {
            let name = rule.exe_name.trim().rsplit(['\\', '/']).next()?.to_lowercase();
            if name.is_empty() {
                return None;
            }
            rule.exe_name = if name.ends_with(".exe") {
                name
            } else {
                format!("{name}.exe")
            };
            seen.insert(rule.exe_name.clone()).then_some(rule)
        })
        .collect()
}

/// First rule in list order wins when several listed apps run at once.
fn matching_rule(rules: &[AppRule], running: &HashSet<String>) -> Option<AppRule> {
    rules
        .iter()
        .find(|rule| running.contains(&rule.exe_name))
        .cloned()
}

pub fn start(app: AppHandle) {
    thread::spawn(move || {
        // None = nothing applied yet this session; Some(None) = defaults applied.
        let mut applied: Option<Option<AppRule>> = None;
        loop {
            thread::sleep(POLL_INTERVAL);
            let rules = app.state::<AppRuleStore>().rules();
            if rules.is_empty() && !matches!(applied, Some(Some(_))) {
                continue;
            }
            let wanted = matching_rule(&rules, &running_exe_names());
            if applied.as_ref() == Some(&wanted) {
                continue;
            }

            let controls = app.state::<BackendState>().controls();
            let (power, fan) = match &wanted {
                Some(rule) => (rule.power_profile.clone(), rule.fan_profile.clone()),
                None => (
                    controls.active_power_profile.clone(),
                    controls.active_fan_profile.clone(),
                ),
            };
            // A failed apply (service not up yet) leaves `applied` alone so the next tick retries.
            match apply_profiles(&controls, power, fan) {
                Ok(()) => {
                    match &wanted {
                        Some(rule) => log::info!("App rule for {} applied.", rule.exe_name),
                        None => log::info!("App rules idle; restored default power and fan."),
                    }
                    applied = Some(wanted);
                }
                Err(error) => log::warn!("App rule apply failed: {error}"),
            }
        }
    });
}

fn apply_profiles(
    controls: &ControlSnapshot,
    power: PowerProfileId,
    fan: FanProfileId,
) -> Result<(), DynError> {
    let (processor_state, custom_base) = match power {
        PowerProfileId::Custom => (
            controls.custom_processor_state.clone(),
            Some(controls.custom_power_base.clone()),
        ),
        ref profile => (tray::default_processor_state_for_profile(profile), None),
    };
    service_pipe::apply_power_profile(
        power,
        processor_state,
        custom_base,
        controls.personal_settings.processor_state_control_enabled,
    )?;
    match fan {
        FanProfileId::Custom => {
            service_pipe::apply_custom_fan_curves(controls.fan_curves.clone())?;
        }
        profile => {
            service_pipe::apply_fan_profile(profile)?;
        }
    }
    Ok(())
}

#[cfg(windows)]
fn running_exe_names() -> HashSet<String> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
        System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
            TH32CS_SNAPPROCESS,
        },
    };

    let mut names = HashSet::new();
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return names;
    }
    let mut entry = unsafe { std::mem::zeroed::<PROCESSENTRY32W>() };
    entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
    let mut has_entry = unsafe { Process32FirstW(snapshot, &mut entry) != 0 };
    while has_entry {
        names.insert(super::nitro_guard::wide_to_string(&entry.szExeFile).to_lowercase());
        has_entry = unsafe { Process32NextW(snapshot, &mut entry) != 0 };
    }
    unsafe {
        CloseHandle(snapshot);
    }
    names
}

#[cfg(not(windows))]
fn running_exe_names() -> HashSet<String> {
    HashSet::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(exe_name: &str, power_profile: PowerProfileId) -> AppRule {
        AppRule {
            exe_name: exe_name.into(),
            power_profile,
            fan_profile: FanProfileId::Max,
        }
    }

    #[test]
    fn normalize_accepts_bare_names_and_paths_and_drops_duplicates() {
        let rules = normalize_rules(vec![
            rule("  Cyberpunk2077 ", PowerProfileId::Turbo),
            rule(r"C:\Games\Cyberpunk2077.EXE", PowerProfileId::Balanced),
            rule("", PowerProfileId::Turbo),
            rule("obs64.exe", PowerProfileId::Performance),
        ]);
        let names: Vec<_> = rules.iter().map(|rule| rule.exe_name.as_str()).collect();
        assert_eq!(names, ["cyberpunk2077.exe", "obs64.exe"]);
        assert_eq!(rules[0].power_profile, PowerProfileId::Turbo);
    }

    #[test]
    fn first_listed_running_app_wins() {
        let rules = vec![
            rule("game.exe", PowerProfileId::Turbo),
            rule("obs64.exe", PowerProfileId::Performance),
        ];
        let running: HashSet<String> = ["obs64.exe".into(), "game.exe".into()].into();
        assert_eq!(
            matching_rule(&rules, &running).unwrap().exe_name,
            "game.exe"
        );
        assert!(matching_rule(&rules, &HashSet::new()).is_none());
    }
}
