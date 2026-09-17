use std::{
    fs,
    os::windows::io::{FromRawHandle, OwnedHandle},
    sync::atomic::{AtomicBool, Ordering},
};

use windows_sys::Win32::{
    Foundation::{
        CloseHandle, LocalFree, ERROR_ACCESS_DENIED, ERROR_PIPE_CONNECTED, HLOCAL,
        INVALID_HANDLE_VALUE,
    },
    Security::{
        Authorization::{ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1},
        SECURITY_ATTRIBUTES,
    },
    Storage::FileSystem::{FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX},
    System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE,
        PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
    },
};

use crate::paths::{write_log_line, ServicePaths};

const PIPE_SDDL: &str = "D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;AU)";

/// Set once this process has successfully created the first instance. The
/// FIRST_PIPE_INSTANCE flag rejects *any* second instance, including our own
/// concurrent ones, so it is only used to claim the name, not on every create.
static PIPE_NAME_OWNED: AtomicBool = AtomicBool::new(false);

pub fn create_pipe_instance(
    paths: &ServicePaths,
    pipe_path: &str,
) -> Result<NamedPipeInstance, Box<dyn std::error::Error + Send + Sync>> {
    let name = to_wide_string(pipe_path);
    let security = PipeSecurityDescriptor::new(PIPE_SDDL)?;
    let claiming_name = !PIPE_NAME_OWNED.load(Ordering::SeqCst);
    let open_mode = if claiming_name {
        PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE
    } else {
        PIPE_ACCESS_DUPLEX
    };
    let handle = unsafe {
        CreateNamedPipeW(
            name.as_ptr(),
            open_mode,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
            PIPE_UNLIMITED_INSTANCES,
            16 * 1024,
            16 * 1024,
            0,
            security.attributes(),
        )
    };

    if handle == INVALID_HANDLE_VALUE {
        let error = std::io::Error::last_os_error();
        if claiming_name && error.raw_os_error() == Some(ERROR_ACCESS_DENIED as i32) {
            // Another process already owns this pipe name. Sharing it would let
            // that process receive the desktop app's requests (and its write
            // token), so refuse and let the supervisor retry after its delay.
            let _ = write_log_line(
                &paths.component_log("ipc-transport"),
                "ERROR",
                "Named pipe name is already owned by another process; refusing to share it.",
            );
        }
        return Err(error.into());
    }

    PIPE_NAME_OWNED.store(true, Ordering::SeqCst);
    Ok(NamedPipeInstance { handle })
}

pub fn connect_client(
    pipe: &NamedPipeInstance,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let connected = unsafe { ConnectNamedPipe(pipe.as_raw_handle(), std::ptr::null_mut()) };
    if connected == 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() != Some(ERROR_PIPE_CONNECTED as i32) {
            return Err(error.into());
        }
    }

    Ok(())
}

fn to_wide_string(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

pub struct NamedPipeInstance {
    handle: windows_sys::Win32::Foundation::HANDLE,
}

unsafe impl Send for NamedPipeInstance {}

struct PipeSecurityDescriptor {
    attributes: SECURITY_ATTRIBUTES,
    descriptor: *mut core::ffi::c_void,
}

impl PipeSecurityDescriptor {
    fn new(sddl: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let mut descriptor: *mut core::ffi::c_void = std::ptr::null_mut();
        let sddl_wide = to_wide_string(sddl);
        let converted = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl_wide.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                std::ptr::null_mut(),
            )
        };

        if converted == 0 {
            return Err(std::io::Error::last_os_error().into());
        }

        Ok(Self {
            attributes: SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: descriptor.cast(),
                bInheritHandle: 0,
            },
            descriptor,
        })
    }

    fn attributes(&self) -> *const SECURITY_ATTRIBUTES {
        &self.attributes
    }
}

impl Drop for PipeSecurityDescriptor {
    fn drop(&mut self) {
        if !self.descriptor.is_null() {
            unsafe {
                let _ = LocalFree(self.descriptor as HLOCAL);
            }
        }
    }
}

impl NamedPipeInstance {
    pub fn into_file(self) -> fs::File {
        let handle = self.handle;
        std::mem::forget(self);
        unsafe {
            let owned = OwnedHandle::from_raw_handle(handle as _);
            fs::File::from(owned)
        }
    }

    fn as_raw_handle(&self) -> windows_sys::Win32::Foundation::HANDLE {
        self.handle
    }
}

impl Drop for NamedPipeInstance {
    fn drop(&mut self) {
        unsafe {
            let _ = DisconnectNamedPipe(self.handle);
            let _ = CloseHandle(self.handle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::create_pipe_instance;
    use crate::paths::ServicePaths;

    #[test]
    fn a_second_instance_of_our_own_pipe_is_allowed_while_the_first_is_open() {
        let root = std::env::temp_dir().join(format!("aeroforge-pipe-test-{}", std::process::id()));
        let paths = ServicePaths::from_root(root.clone()).unwrap();
        let name = format!(r"\\.\pipe\AeroForgeServiceTest{}", std::process::id());

        // The accept loop always creates the next instance while a handler
        // thread still holds the previous one; FIRST_PIPE_INSTANCE on every
        // create would reject this with ERROR_ACCESS_DENIED.
        let first = create_pipe_instance(&paths, &name).expect("first instance");
        let second = create_pipe_instance(&paths, &name).expect("second instance while first open");
        drop(second);
        drop(first);
        let _ = std::fs::remove_dir_all(root);
    }
}
