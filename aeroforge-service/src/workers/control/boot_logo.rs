use std::path::Path;

use crate::paths::{write_log_line, ServicePaths};

use super::models::{AppliedBootLogoSnapshot, ApplyBootLogoRequest};

const MAX_BOOT_LOGO_BYTES: u64 = 35 * 1024 * 1024;
const DIRECT_BOOT_LOGO_UNAVAILABLE: &str = "Boot-logo firmware apply is disabled because AeroForge does not yet have a verified direct hardware path. The previous Acer service route has been removed.";

pub fn apply_boot_logo(
    paths: &ServicePaths,
    request: ApplyBootLogoRequest,
) -> Result<AppliedBootLogoSnapshot, Box<dyn std::error::Error + Send + Sync>> {
    validate_boot_logo_file(&request.image_path)?;

    let detail = format!(
        "{} Staged image: {}{}.",
        DIRECT_BOOT_LOGO_UNAVAILABLE,
        request.image_path,
        request
            .original_filename
            .as_deref()
            .map(|name| format!(" ({name})"))
            .unwrap_or_default()
    );
    write_log_line(&paths.component_log("control-boot-logo"), "WARN", &detail)?;

    Err(DIRECT_BOOT_LOGO_UNAVAILABLE.into())
}

fn validate_boot_logo_file(path: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let path_ref = Path::new(path);
    if !path_ref.exists() {
        return Err(format!("Boot-logo image does not exist: {path}").into());
    }
    if !path_ref.is_file() {
        return Err(format!("Boot-logo image path is not a file: {path}").into());
    }

    let extension = path_ref
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !matches!(extension.as_str(), "jpg" | "jpeg" | "gif") {
        return Err("Boot-logo image must be JPG, JPEG, or GIF before firmware apply.".into());
    }

    let size = std::fs::metadata(path_ref)?.len();
    if size > MAX_BOOT_LOGO_BYTES {
        return Err(format!(
            "Boot-logo image is too large: {size} bytes, max {MAX_BOOT_LOGO_BYTES} bytes."
        )
        .into());
    }

    Ok(())
}
