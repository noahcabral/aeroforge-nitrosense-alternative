use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    path::Path,
    process::Command,
    thread,
    time::{Duration, Instant},
};

use serde_json::{json, Value};

use crate::{
    paths::{write_log_line, ServicePaths},
    workers::unix_timestamp,
};

use super::models::{AppliedBootLogoSnapshot, ApplyBootLogoRequest};

const ACER_AGENT_SERVICE: &str = "AASSvc";
const ACER_AGENT_ADDR: &str = "127.0.0.1:46933";
const CUSTOM_BOOT_LOGO: &str = "CUSTOM_BOOT_LOGO";
const SUPPORT_CUSTOM_BOOT_LOGO: &str = "SUPPORT_CUSTOM_BOOT_LOGO";
const MAX_BOOT_LOGO_BYTES: u64 = 35 * 1024 * 1024;

const PACKET_INITIALIZATION: u32 = 0;
const PACKET_GET_UPDATED_DATA: u32 = 20;
const PACKET_SET_DEVICE_DATA: u32 = 100;

pub fn apply_boot_logo(
    paths: &ServicePaths,
    request: ApplyBootLogoRequest,
) -> Result<AppliedBootLogoSnapshot, Box<dyn std::error::Error + Send + Sync>> {
    validate_boot_logo_file(&request.image_path)?;

    write_log_line(
        &paths.component_log("control-boot-logo"),
        "INFO",
        &format!(
            "Applying boot logo from {}{}.",
            request.image_path,
            request
                .original_filename
                .as_deref()
                .map(|name| format!(" ({name})"))
                .unwrap_or_default()
        ),
    )?;

    let mut lifecycle = AcerAgentLifecycle::ensure_available(paths)?;
    let support_response = send_pssdk_request(
        PACKET_INITIALIZATION,
        json!({
            "Function": SUPPORT_CUSTOM_BOOT_LOGO,
        }),
    )
    .ok();

    let set_response = send_pssdk_request(
        PACKET_SET_DEVICE_DATA,
        json!({
            "Function": CUSTOM_BOOT_LOGO,
            "Parameter": {
                "imageSource": request.image_path,
            },
        }),
    )?;

    if let Some(result) = response_result_code(&set_response) {
        if result != 0 {
            lifecycle.restore(paths);
            return Err(format!(
                "AcerAgentService rejected the boot-logo image with result {result}: {set_response}"
            )
            .into());
        }
    }

    let get_response = send_pssdk_request(
        PACKET_GET_UPDATED_DATA,
        json!({
            "Function": CUSTOM_BOOT_LOGO,
        }),
    )
    .ok();

    let readback = json!({
        "backend": "acer-agent-service-pssdk",
        "address": ACER_AGENT_ADDR,
        "service": ACER_AGENT_SERVICE,
        "supportResponse": support_response,
        "setResponse": set_response,
        "getUpdatedDataResponse": get_response,
        "serviceLifecycle": lifecycle.detail(),
    });

    lifecycle.restore(paths);

    let detail = "Boot logo write accepted by AcerAgentService. Restart Windows for firmware splash changes to become visible.".to_string();
    write_log_line(&paths.component_log("control-boot-logo"), "INFO", &detail)?;

    Ok(AppliedBootLogoSnapshot {
        image_path: request.image_path,
        original_filename: request.original_filename,
        readback: Some(readback),
        applied_at_unix: unix_timestamp(),
        detail,
    })
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

fn response_result_code(response: &Value) -> Option<i64> {
    response
        .get("result")
        .and_then(|value| value.as_i64().or_else(|| value.as_str()?.parse::<i64>().ok()))
}

fn send_pssdk_request(
    packet_id: u32,
    payload: Value,
) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
    let address: SocketAddr = ACER_AGENT_ADDR.parse()?;
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(2))?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;

    let payload = serde_json::to_vec(&payload)?;
    let mut frame = Vec::with_capacity(8 + payload.len());
    frame.extend_from_slice(b"ACER");
    frame.extend_from_slice(&packet_id.to_le_bytes());
    frame.extend_from_slice(&payload);

    stream.write_all(&frame)?;
    stream.flush()?;

    read_first_json_response(&mut stream)
}

fn read_first_json_response(
    stream: &mut TcpStream,
) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
    let started = Instant::now();
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 4096];

    loop {
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => {
                bytes.extend_from_slice(&buffer[..read]);
                let text = String::from_utf8_lossy(&bytes);
                if let Some(json_text) = extract_first_json_object(&text) {
                    return Ok(serde_json::from_str(json_text)?);
                }
            }
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock
                    || error.kind() == std::io::ErrorKind::TimedOut =>
            {
                if !bytes.is_empty() || started.elapsed() >= Duration::from_secs(5) {
                    break;
                }
            }
            Err(error) => return Err(error.into()),
        }

        if started.elapsed() >= Duration::from_secs(5) {
            break;
        }
    }

    let text = String::from_utf8_lossy(&bytes);
    if text.trim().is_empty() {
        return Err("AcerAgentService returned no boot-logo response.".into());
    }

    Err(format!("AcerAgentService returned an unparsable boot-logo response: {text}").into())
}

fn extract_first_json_object(text: &str) -> Option<&str> {
    let mut start = None;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    for (index, ch) in text.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '{' => {
                if depth == 0 {
                    start = Some(index);
                }
                depth += 1;
            }
            '}' => {
                if depth == 0 {
                    continue;
                }
                depth -= 1;
                if depth == 0 {
                    return start.map(|start_index| &text[start_index..=index]);
                }
            }
            _ => {}
        }
    }

    None
}

struct AcerAgentLifecycle {
    started_by_us: bool,
    startup_was_disabled: bool,
    restored: bool,
}

impl AcerAgentLifecycle {
    fn ensure_available(
        paths: &ServicePaths,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        if port_is_listening() {
            return Ok(Self {
                started_by_us: false,
                startup_was_disabled: false,
                restored: true,
            });
        }

        let startup_was_disabled = service_startup_is_disabled().unwrap_or(false);
        if startup_was_disabled {
            run_sc(&["config", ACER_AGENT_SERVICE, "start=", "demand"])?;
        }

        let _ = run_sc(&["start", ACER_AGENT_SERVICE]);

        let deadline = Instant::now() + Duration::from_secs(8);
        while Instant::now() < deadline {
            if port_is_listening() {
                return Ok(Self {
                    started_by_us: true,
                    startup_was_disabled,
                    restored: false,
                });
            }
            thread::sleep(Duration::from_millis(250));
        }

        if startup_was_disabled {
            let _ = run_sc(&["config", ACER_AGENT_SERVICE, "start=", "disabled"]);
        }

        let message =
            "AcerAgentService did not open the CUSTOM_BOOT_LOGO socket on 127.0.0.1:46933.";
        let _ = write_log_line(&paths.component_log("control-boot-logo"), "ERROR", message);
        Err(message.into())
    }

    fn restore(&mut self, paths: &ServicePaths) {
        if self.restored {
            return;
        }

        if self.started_by_us {
            let _ = run_sc(&["stop", ACER_AGENT_SERVICE]);
        }
        if self.startup_was_disabled {
            let _ = run_sc(&["config", ACER_AGENT_SERVICE, "start=", "disabled"]);
        }

        self.restored = true;
        let _ = write_log_line(
            &paths.component_log("control-boot-logo"),
            "INFO",
            "Restored Acer Agent Service lifecycle after boot-logo apply.",
        );
    }

    fn detail(&self) -> Value {
        json!({
            "startedByAeroForge": self.started_by_us,
            "startupWasDisabled": self.startup_was_disabled,
            "restored": self.restored,
        })
    }
}

impl Drop for AcerAgentLifecycle {
    fn drop(&mut self) {
        if !self.restored {
            if self.started_by_us {
                let _ = run_sc(&["stop", ACER_AGENT_SERVICE]);
            }
            if self.startup_was_disabled {
                let _ = run_sc(&["config", ACER_AGENT_SERVICE, "start=", "disabled"]);
            }
            self.restored = true;
        }
    }
}

fn port_is_listening() -> bool {
    let Ok(address) = ACER_AGENT_ADDR.parse::<SocketAddr>() else {
        return false;
    };

    TcpStream::connect_timeout(&address, Duration::from_millis(300)).is_ok()
}

fn service_startup_is_disabled() -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    let output = run_sc(&["qc", ACER_AGENT_SERVICE])?;
    Ok(output.contains("DISABLED"))
}

fn run_sc(args: &[&str]) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let output = Command::new("sc.exe").args(args).output()?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    if output.status.success() {
        Ok(text)
    } else {
        Err(format!("sc.exe {} failed: {text}", args.join(" ")).into())
    }
}
