use std::{
    fs,
    path::{Path, PathBuf},
};

use base64::{engine::general_purpose::STANDARD, Engine as _};

const MAX_BOOT_LOGO_BYTES: usize = 35 * 1024 * 1024;

pub fn save_uploaded_boot_logo(
    config_root: &Path,
    file_name: &str,
    image_base64: &str,
) -> Result<PathBuf, Box<dyn std::error::Error + Send + Sync>> {
    let payload = image_base64
        .split_once(',')
        .map(|(_, data)| data)
        .unwrap_or(image_base64)
        .trim();
    let bytes = STANDARD.decode(payload)?;

    if bytes.is_empty() {
        return Err("Boot-logo upload was empty.".into());
    }
    if bytes.len() > MAX_BOOT_LOGO_BYTES {
        return Err(format!(
            "Boot-logo upload is too large: {} bytes, max {} bytes.",
            bytes.len(),
            MAX_BOOT_LOGO_BYTES
        )
        .into());
    }

    let boot_logo_dir = config_root.join("boot-logo");
    fs::create_dir_all(&boot_logo_dir)?;

    let safe_stem = sanitize_file_stem(file_name);
    let output_path = boot_logo_dir.join(format!("{safe_stem}.jpg"));
    fs::write(&output_path, bytes)?;

    Ok(output_path)
}

fn sanitize_file_stem(file_name: &str) -> String {
    let stem = Path::new(file_name)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("aeroforge-boot-logo");

    let mut sanitized = String::new();
    for ch in stem.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
            sanitized.push(ch);
        } else if ch.is_whitespace() {
            sanitized.push('-');
        }
    }

    if sanitized.is_empty() {
        "aeroforge-boot-logo".into()
    } else {
        sanitized
    }
}
