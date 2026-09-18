//! Size-capped append-only text logs shared by the app and the hotkey helper,
//! both of which run for the life of the session and log every Nitro key press.
use std::{
    fs, io,
    io::Write,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_LOG_BYTES: u64 = 2 * 1024 * 1024;

/// Moves `path` aside as `<file name>.old` once it reaches the cap, keeping one
/// generation so a long-running process cannot fill the profile directory.
pub fn rotate_if_needed(path: &Path) -> io::Result<()> {
    let Ok(metadata) = fs::metadata(path) else {
        return Ok(());
    };
    if metadata.len() < MAX_LOG_BYTES {
        return Ok(());
    }
    let mut archived = path.as_os_str().to_owned();
    archived.push(".old");
    let _ = fs::remove_file(&archived);
    fs::rename(path, archived)
}

/// Best-effort `<unix seconds> <message>` line; logging never fails the caller.
pub fn append_line(path: &Path, message: &str) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = rotate_if_needed(path);
    if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(path) {
        let seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or(0);
        let _ = writeln!(file, "{seconds} {message}");
    }
}

#[cfg(test)]
mod tests {
    use super::{append_line, MAX_LOG_BYTES};
    use std::fs;

    #[test]
    fn append_line_rotates_once_the_cap_is_reached() {
        let dir = std::env::temp_dir().join(format!("aeroforge-log-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join("nested").join("helper.log");
        append_line(&path, "first");
        let first = fs::read_to_string(&path).expect("log");
        assert!(first.trim_end().ends_with(" first"), "{first:?}");

        fs::write(&path, vec![b'x'; MAX_LOG_BYTES as usize]).expect("fill");
        append_line(&path, "second");
        assert_eq!(
            fs::metadata(path.with_extension("log.old"))
                .expect("rotated")
                .len(),
            MAX_LOG_BYTES
        );
        let second = fs::read_to_string(&path).expect("fresh log");
        assert!(second.trim_end().ends_with(" second"), "{second:?}");
        let _ = fs::remove_dir_all(&dir);
    }
}
