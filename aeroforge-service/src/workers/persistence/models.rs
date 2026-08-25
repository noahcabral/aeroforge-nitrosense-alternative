use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistenceSnapshot {
    pub service: &'static str,
    pub tauri_control_state_path: String,
    pub tauri_control_state_present: bool,
    pub service_state_root: String,
    pub next_phase: &'static str,
}
