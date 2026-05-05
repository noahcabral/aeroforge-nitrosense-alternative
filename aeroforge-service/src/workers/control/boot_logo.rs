use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

use serde_json::json;

use crate::{
    paths::{write_log_line, ServicePaths},
    workers::unix_timestamp,
};

use super::models::{AppliedBootLogoSnapshot, ApplyBootLogoRequest};

const MAX_BOOT_LOGO_BYTES: u64 = 16 * 1024 * 1024;
const MIN_ESP_FREE_AFTER_WRITE_BYTES: u64 = 16 * 1024 * 1024;

pub fn apply_boot_logo(
    paths: &ServicePaths,
    request: ApplyBootLogoRequest,
) -> Result<AppliedBootLogoSnapshot, Box<dyn std::error::Error + Send + Sync>> {
    let staged = validate_boot_logo_file(&request.image_path)?;
    let esp = platform::locate_efi_system_partition()?;

    let required_free = staged.size + MIN_ESP_FREE_AFTER_WRITE_BYTES;
    if esp.free_bytes < required_free {
        return Err(format!(
            "EFI partition does not have enough free space for a safe boot-logo write. Free {} bytes, need at least {} bytes.",
            esp.free_bytes, required_free
        )
        .into());
    }

    let write_result = write_logo_to_esp(paths, &staged, &esp)?;
    let detail = format!(
        "Boot logo staged in Acer program data at {}, Acer recovery at {}, and on the EFI System Partition as {}. Backup: {}. Reboot is required to verify firmware pickup.",
        write_result.acer_programdata_target_path.display(),
        write_result.acer_recovery_target_path.display(),
        write_result.target_path.display(),
        write_result
            .backup_dir
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "not needed; no previous AcerLogo files were present".into())
    );
    write_log_line(&paths.component_log("control-boot-logo"), "INFO", &detail)?;

    Ok(AppliedBootLogoSnapshot {
        image_path: request.image_path,
        original_filename: request.original_filename,
        readback: Some(json!({
            "backend": "direct-efi-file",
            "efiVolume": esp.volume_name,
            "efiLabel": esp.label,
            "efiFileSystem": esp.file_system,
            "efiTotalBytes": esp.total_bytes,
            "efiFreeBytesBeforeWrite": esp.free_bytes,
            "efiMarker": esp.marker_path.display().to_string(),
            "targetPath": write_result.target_path.display().to_string(),
            "targetBytes": write_result.target_size,
            "acerProgramDataTargetPath": write_result.acer_programdata_target_path.display().to_string(),
            "acerRecoveryTargetPath": write_result.acer_recovery_target_path.display().to_string(),
            "targetFormat": staged.format.as_str(),
            "alternateRemoved": write_result.alternate_removed,
            "backupDir": write_result
                .backup_dir
                .as_ref()
                .map(|path| path.display().to_string()),
        })),
        applied_at_unix: unix_timestamp(),
        detail,
    })
}

fn validate_boot_logo_file(
    path: &str,
) -> Result<ValidatedBootLogo, Box<dyn std::error::Error + Send + Sync>> {
    let path_ref = Path::new(path);
    if !path_ref.exists() {
        return Err(format!("Boot-logo image does not exist: {path}").into());
    }
    if !path_ref.is_file() {
        return Err(format!("Boot-logo image path is not a file: {path}").into());
    }

    let size = fs::metadata(path_ref)?.len();
    if size == 0 {
        return Err("Boot-logo image is empty.".into());
    }
    if size > MAX_BOOT_LOGO_BYTES {
        return Err(format!(
            "Boot-logo image is too large: {size} bytes, max {MAX_BOOT_LOGO_BYTES} bytes."
        )
        .into());
    }

    let extension = path_ref
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let mut header = [0u8; 12];
    let mut file = File::open(path_ref)?;
    let header_len = file.read(&mut header)?;
    let format = BootLogoFormat::from_extension_and_header(&extension, &header[..header_len])?;

    Ok(ValidatedBootLogo {
        path: path_ref.to_path_buf(),
        format,
        size,
    })
}

fn write_logo_to_esp(
    paths: &ServicePaths,
    staged: &ValidatedBootLogo,
    esp: &platform::EspVolume,
) -> Result<BootLogoWriteResult, Box<dyn std::error::Error + Send + Sync>> {
    let acer_programdata_dir = PathBuf::from(r"C:\ProgramData\oem\AcerAgentService\BIOSLogo");
    let acer_recovery_dir = PathBuf::from(r"C:\Recovery\OEM\BiosAnimation");
    let acer_programdata_stage =
        stage_logo_pair(paths, staged, &acer_programdata_dir, "acer-programdata")?;
    let acer_recovery_stage = stage_logo_pair(paths, staged, &acer_recovery_dir, "acer-recovery")?;

    let target_dir = esp.root.join("EFI").join("OEM");
    ensure_path_stays_under(&target_dir, &esp.root)?;
    fs::create_dir_all(&target_dir)?;

    let target_path = target_dir.join(staged.format.target_file_name());
    let alternate_path = target_dir.join(staged.format.alternate_file_name());
    let temp_path = target_dir.join(format!(
        ".{}.aeroforge-write",
        staged.format.target_file_name()
    ));

    remove_file_if_exists(&temp_path)?;
    let backup_dir = backup_existing_logos(paths, &target_dir)?;

    copy_file_synced(&staged.path, &temp_path)?;
    let temp_size = fs::metadata(&temp_path)?.len();
    if temp_size != staged.size {
        remove_file_if_exists(&temp_path)?;
        return Err(format!(
            "Boot-logo temp copy verification failed. Source {} bytes, temp {} bytes.",
            staged.size, temp_size
        )
        .into());
    }

    let alternate_removed = remove_file_if_exists(&alternate_path)?;
    remove_file_if_exists(&target_path)?;

    if let Err(error) = fs::rename(&temp_path, &target_path) {
        let rollback_error = rollback_from_backup(&target_dir, backup_dir.as_deref());
        let rollback_detail = rollback_error
            .map(|_| "rollback completed".into())
            .unwrap_or_else(|err| format!("rollback failed: {err}"));
        return Err(format!("Boot-logo EFI rename failed: {error}. {rollback_detail}.").into());
    }

    let target_size = fs::metadata(&target_path)?.len();
    if target_size != staged.size {
        return Err(format!(
            "Boot-logo target verification failed. Source {} bytes, target {} bytes.",
            staged.size, target_size
        )
        .into());
    }

    Ok(BootLogoWriteResult {
        target_path,
        target_size,
        alternate_removed,
        backup_dir,
        acer_programdata_target_path: acer_programdata_stage.target_path,
        acer_recovery_target_path: acer_recovery_stage.target_path,
    })
}

fn backup_existing_logos(
    paths: &ServicePaths,
    target_dir: &Path,
) -> Result<Option<PathBuf>, Box<dyn std::error::Error + Send + Sync>> {
    let existing = ["AcerLogo.jpg", "AcerLogo.gif"]
        .into_iter()
        .map(|name| target_dir.join(name))
        .filter(|path| match fs::metadata(path) {
            Ok(metadata) => metadata.is_file(),
            Err(error) if error.kind() == io::ErrorKind::NotFound => false,
            Err(_) => true,
        })
        .collect::<Vec<_>>();

    if existing.is_empty() {
        return Ok(None);
    }

    let backup_dir = paths
        .state_dir
        .join("boot-logo-backups")
        .join(unix_timestamp().to_string());
    fs::create_dir_all(&backup_dir)?;

    for source in existing {
        if !fs::metadata(&source)?.is_file() {
            return Err(format!(
                "Refusing to back up non-file boot-logo path: {}",
                source.display()
            )
            .into());
        }
        let destination = backup_dir.join(source.file_name().ok_or("Invalid boot-logo file name")?);
        copy_file_synced(&source, &destination)?;
    }

    Ok(Some(backup_dir))
}

fn rollback_from_backup(target_dir: &Path, backup_dir: Option<&Path>) -> Result<(), io::Error> {
    remove_file_if_exists(&target_dir.join("AcerLogo.jpg"))?;
    remove_file_if_exists(&target_dir.join("AcerLogo.gif"))?;

    let Some(backup_dir) = backup_dir else {
        return Ok(());
    };

    for name in ["AcerLogo.jpg", "AcerLogo.gif"] {
        let backup = backup_dir.join(name);
        if file_exists(&backup)? {
            copy_file_synced(&backup, &target_dir.join(name))?;
        }
    }

    Ok(())
}

fn copy_file_synced(source: &Path, destination: &Path) -> Result<(), io::Error> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut input = File::open(source)?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)?;
    io::copy(&mut input, &mut output)?;
    output.flush()?;
    output.sync_all()?;
    Ok(())
}

fn remove_file_if_exists(path: &Path) -> Result<bool, io::Error> {
    match fs::metadata(path) {
        Ok(metadata) => {
            if !metadata.is_file() {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    format!("Refusing to remove non-file path: {}", path.display()),
                ));
            }
            fs::remove_file(path)?;
            Ok(true)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn file_exists(path: &Path) -> Result<bool, io::Error> {
    match fs::metadata(path) {
        Ok(metadata) => Ok(metadata.is_file()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn ensure_path_stays_under(
    path: &Path,
    root: &Path,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if path.starts_with(root) {
        Ok(())
    } else {
        Err(format!(
            "Refusing EFI write because target {} is outside discovered EFI root {}.",
            path.display(),
            root.display()
        )
        .into())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BootLogoFormat {
    Gif,
    Jpeg,
}

impl BootLogoFormat {
    fn from_extension_and_header(
        extension: &str,
        header: &[u8],
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        match extension {
            "gif" if header.starts_with(b"GIF87a") || header.starts_with(b"GIF89a") => {
                Ok(Self::Gif)
            }
            "jpg" | "jpeg" if header.starts_with(&[0xFF, 0xD8, 0xFF]) => Ok(Self::Jpeg),
            "gif" => Err("Boot-logo GIF extension did not match GIF file header.".into()),
            "jpg" | "jpeg" => Err("Boot-logo JPG extension did not match JPEG file header.".into()),
            _ => Err("Boot-logo image must be JPG, JPEG, or GIF before firmware apply.".into()),
        }
    }

    fn target_file_name(self) -> &'static str {
        match self {
            Self::Gif => "AcerLogo.gif",
            Self::Jpeg => "AcerLogo.jpg",
        }
    }

    fn alternate_file_name(self) -> &'static str {
        match self {
            Self::Gif => "AcerLogo.jpg",
            Self::Jpeg => "AcerLogo.gif",
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Gif => "gif",
            Self::Jpeg => "jpg",
        }
    }
}

struct ValidatedBootLogo {
    path: PathBuf,
    format: BootLogoFormat,
    size: u64,
}

struct BootLogoWriteResult {
    target_path: PathBuf,
    target_size: u64,
    alternate_removed: bool,
    backup_dir: Option<PathBuf>,
    acer_programdata_target_path: PathBuf,
    acer_recovery_target_path: PathBuf,
}

fn stage_logo_pair(
    paths: &ServicePaths,
    staged: &ValidatedBootLogo,
    target_dir: &Path,
    stage_name: &str,
) -> Result<StageWriteResult, Box<dyn std::error::Error + Send + Sync>> {
    fs::create_dir_all(target_dir)?;
    let target_path = target_dir.join(staged.format.target_file_name());
    let alternate_path = target_dir.join(staged.format.alternate_file_name());
    let temp_path = target_dir.join(format!(
        ".{}.aeroforge-{}-write",
        staged.format.target_file_name(),
        stage_name
    ));

    remove_file_if_exists(&temp_path)?;
    backup_existing_logo_pair(paths, target_dir, stage_name)?;
    copy_file_synced(&staged.path, &temp_path)?;
    let temp_size = fs::metadata(&temp_path)?.len();
    if temp_size != staged.size {
        remove_file_if_exists(&temp_path)?;
        return Err(format!(
            "Boot-logo {stage_name} temp copy verification failed. Source {} bytes, temp {} bytes.",
            staged.size, temp_size
        )
        .into());
    }

    remove_file_if_exists(&alternate_path)?;
    remove_file_if_exists(&target_path)?;
    fs::rename(&temp_path, &target_path)?;

    let target_size = fs::metadata(&target_path)?.len();
    if target_size != staged.size {
        return Err(format!(
            "Boot-logo {stage_name} verification failed. Source {} bytes, target {} bytes.",
            staged.size, target_size
        )
        .into());
    }

    Ok(StageWriteResult { target_path })
}

fn backup_existing_logo_pair(
    paths: &ServicePaths,
    target_dir: &Path,
    stage_name: &str,
) -> Result<Option<PathBuf>, Box<dyn std::error::Error + Send + Sync>> {
    let existing = ["AcerLogo.jpg", "AcerLogo.gif"]
        .into_iter()
        .map(|name| target_dir.join(name))
        .filter(|path| match fs::metadata(path) {
            Ok(metadata) => metadata.is_file(),
            Err(error) if error.kind() == io::ErrorKind::NotFound => false,
            Err(_) => true,
        })
        .collect::<Vec<_>>();

    if existing.is_empty() {
        return Ok(None);
    }

    let backup_dir = paths.state_dir.join("boot-logo-backups").join(format!(
        "{}-{}",
        unix_timestamp(),
        stage_name
    ));
    fs::create_dir_all(&backup_dir)?;

    for source in existing {
        if !fs::metadata(&source)?.is_file() {
            return Err(format!(
                "Refusing to back up non-file boot-logo path: {}",
                source.display()
            )
            .into());
        }
        let destination = backup_dir.join(source.file_name().ok_or("Invalid boot-logo file name")?);
        copy_file_synced(&source, &destination)?;
    }

    Ok(Some(backup_dir))
}

struct StageWriteResult {
    target_path: PathBuf,
}

#[cfg(windows)]
mod platform {
    use std::{
        io,
        path::{Path, PathBuf},
    };

    use windows_sys::Win32::{
        Foundation::INVALID_HANDLE_VALUE,
        Storage::FileSystem::{
            FindFirstVolumeW, FindNextVolumeW, FindVolumeClose, GetDiskFreeSpaceExW,
            GetVolumeInformationW,
        },
    };

    const MIN_ESP_BYTES: u64 = 50 * 1024 * 1024;
    const MAX_ESP_BYTES: u64 = 1024 * 1024 * 1024;

    pub struct EspVolume {
        pub volume_name: String,
        pub root: PathBuf,
        pub label: String,
        pub file_system: String,
        pub total_bytes: u64,
        pub free_bytes: u64,
        pub marker_path: PathBuf,
    }

    pub fn locate_efi_system_partition(
    ) -> Result<EspVolume, Box<dyn std::error::Error + Send + Sync>> {
        let mut volume_names = Vec::new();
        let mut buffer = vec![0u16; 512];

        let handle = unsafe { FindFirstVolumeW(buffer.as_mut_ptr(), buffer.len() as u32) };
        if handle == INVALID_HANDLE_VALUE {
            return Err(format!(
                "Unable to enumerate Windows volumes: {}",
                io::Error::last_os_error()
            )
            .into());
        }

        loop {
            volume_names.push(wide_buffer_to_string(&buffer));
            let ok = unsafe { FindNextVolumeW(handle, buffer.as_mut_ptr(), buffer.len() as u32) };
            if ok == 0 {
                break;
            }
        }

        unsafe {
            FindVolumeClose(handle);
        }

        let mut candidates = Vec::new();
        let mut inspected = 0usize;
        for volume_name in volume_names {
            if volume_name.is_empty() {
                continue;
            }
            inspected += 1;
            match inspect_volume(&volume_name) {
                Ok(Some(candidate)) => candidates.push(candidate),
                Ok(None) => {}
                Err(_) => {}
            }
        }

        match candidates.len() {
            1 => Ok(candidates.remove(0)),
            0 => Err(format!(
                "No safe EFI System Partition candidate found after inspecting {inspected} volumes. AeroForge requires a FAT32 system volume containing Windows EFI boot markers before writing a boot logo."
            )
            .into()),
            _ => Err(format!(
                "Multiple EFI System Partition candidates found ({}). Refusing boot-logo write until the target is unambiguous.",
                candidates.len()
            )
            .into()),
        }
    }

    fn inspect_volume(
        volume_name: &str,
    ) -> Result<Option<EspVolume>, Box<dyn std::error::Error + Send + Sync>> {
        let (label, file_system) = get_volume_information(volume_name)?;
        if !file_system.eq_ignore_ascii_case("FAT32") {
            return Ok(None);
        }

        let (total_bytes, free_bytes) = get_disk_space(volume_name)?;
        if !(MIN_ESP_BYTES..=MAX_ESP_BYTES).contains(&total_bytes) {
            return Ok(None);
        }

        let root = PathBuf::from(volume_name);
        let Some(marker_path) = windows_boot_marker(&root) else {
            return Ok(None);
        };

        Ok(Some(EspVolume {
            volume_name: volume_name.into(),
            root,
            label,
            file_system,
            total_bytes,
            free_bytes,
            marker_path,
        }))
    }

    fn windows_boot_marker(root: &Path) -> Option<PathBuf> {
        [
            root.join("EFI").join("Microsoft").join("Boot").join("BCD"),
            root.join("EFI")
                .join("Microsoft")
                .join("Boot")
                .join("bootmgfw.efi"),
        ]
        .into_iter()
        .find(|path| path.is_file())
    }

    fn get_volume_information(volume_name: &str) -> Result<(String, String), io::Error> {
        let wide = wide_null(volume_name);
        let mut label = vec![0u16; 260];
        let mut file_system = vec![0u16; 64];
        let mut serial = 0u32;
        let mut max_component = 0u32;
        let mut flags = 0u32;

        let ok = unsafe {
            GetVolumeInformationW(
                wide.as_ptr(),
                label.as_mut_ptr(),
                label.len() as u32,
                &mut serial,
                &mut max_component,
                &mut flags,
                file_system.as_mut_ptr(),
                file_system.len() as u32,
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }

        Ok((
            wide_buffer_to_string(&label),
            wide_buffer_to_string(&file_system),
        ))
    }

    fn get_disk_space(volume_name: &str) -> Result<(u64, u64), io::Error> {
        let wide = wide_null(volume_name);
        let mut free_to_caller = 0u64;
        let mut total = 0u64;
        let mut free = 0u64;

        let ok = unsafe {
            GetDiskFreeSpaceExW(wide.as_ptr(), &mut free_to_caller, &mut total, &mut free)
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }

        Ok((total, free))
    }

    fn wide_null(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn wide_buffer_to_string(buffer: &[u16]) -> String {
        let len = buffer
            .iter()
            .position(|value| *value == 0)
            .unwrap_or(buffer.len());
        String::from_utf16_lossy(&buffer[..len])
    }
}

#[cfg(not(windows))]
mod platform {
    use std::path::PathBuf;

    pub struct EspVolume {
        pub volume_name: String,
        pub root: PathBuf,
        pub label: String,
        pub file_system: String,
        pub total_bytes: u64,
        pub free_bytes: u64,
        pub marker_path: PathBuf,
    }

    pub fn locate_efi_system_partition(
    ) -> Result<EspVolume, Box<dyn std::error::Error + Send + Sync>> {
        Err("Direct boot-logo EFI apply is only supported on Windows.".into())
    }
}

#[cfg(test)]
mod tests {
    use super::BootLogoFormat;

    #[test]
    fn detects_jpeg_magic() {
        assert_eq!(
            BootLogoFormat::from_extension_and_header("jpg", &[0xFF, 0xD8, 0xFF, 0xE0]).unwrap(),
            BootLogoFormat::Jpeg
        );
    }

    #[test]
    fn rejects_extension_magic_mismatch() {
        assert!(BootLogoFormat::from_extension_and_header("jpg", b"GIF89a").is_err());
        assert!(BootLogoFormat::from_extension_and_header("gif", &[0xFF, 0xD8, 0xFF]).is_err());
    }

    #[test]
    fn uses_acer_observed_target_names() {
        assert_eq!(BootLogoFormat::Jpeg.target_file_name(), "AcerLogo.jpg");
        assert_eq!(BootLogoFormat::Gif.target_file_name(), "AcerLogo.gif");
    }
}
