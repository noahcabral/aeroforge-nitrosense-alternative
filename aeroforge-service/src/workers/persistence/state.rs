use std::path::PathBuf;

pub fn discover_tauri_state_path() -> PathBuf {
    let program_data = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Users\Default\AppData\Roaming"));

    program_data
        .join("com.noah.aeroforgecontrol")
        .join("control-state.json")
}
