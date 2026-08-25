use std::sync::Arc;

#[cfg(windows)]
pub fn create_single_instance_mutex() -> Result<Arc<windows_sys::Win32::Foundation::HANDLE>, Box<dyn std::error::Error>> {
    use windows_sys::Win32::{
        Foundation::{GetLastError, ERROR_ALREADY_EXISTS, HANDLE},
        System::Threading::CreateMutexW,
    };

    let mutex_name = "AeroForge_Control_SingleInstance\0"
        .encode_utf16()
        .collect::<Vec<u16>>();

    unsafe {
        let handle = CreateMutexW(
            std::ptr::null(),
            1, // bInitialOwner = true
            mutex_name.as_ptr(),
        );

        if handle == 0 {
            return Err("Failed to create single-instance mutex".into());
        }

        if GetLastError() == ERROR_ALREADY_EXISTS {
            return Err("AeroForge Control is already running".into());
        }

        Ok(Arc::new(handle))
    }
}

#[cfg(not(windows))]
pub fn create_single_instance_mutex() -> Result<Arc<()>, Box<dyn std::error::Error>> {
    Ok(Arc::new(()))
}

#[cfg(windows)]
pub fn setup_nitro_key_listener(_ui_weak: slint::Weak<crate::AppWindow>) {
    // Nitro key listener (Win+N) - stub for now
    // Full implementation would use RegisterHotKey Win32 API
    println!("Nitro key listener setup (Win+N hotkey not yet implemented)");
}

#[cfg(not(windows))]
pub fn setup_nitro_key_listener(_ui_weak: slint::Weak<crate::AppWindow>) {
    // No-op on non-Windows platforms
}
