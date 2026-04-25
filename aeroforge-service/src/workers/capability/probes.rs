use std::path::Path;

pub fn nvml_present() -> bool {
    Path::new(r"C:\Windows\System32\nvml.dll").exists()
}
