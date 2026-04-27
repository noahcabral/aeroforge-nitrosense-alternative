use std::{
    fs::OpenOptions,
    io::{BufRead, BufReader, Write},
    thread,
    time::Duration,
};

use serde_json::json;

const PIPE_PATH: &str = r"\\.\pipe\AeroForgeService";

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let profile_id = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "custom".to_string());
    let (min_percent, max_percent) = match profile_id.as_str() {
        "battery-guard" => (5, 45),
        "balanced" => (35, 88),
        "performance" => (100, 100),
        "turbo" => (100, 100),
        _ => (42, 77),
    };
    let custom_base_profile = std::env::args().nth(2);

    let request = json!({
        "kind": "applyPowerProfile",
        "payload": {
            "profileId": profile_id,
            "processorState": {
                "minPercent": min_percent,
                "maxPercent": max_percent
            },
            "customBaseProfile": custom_base_profile
        }
    });

    let mut pipe = open_pipe_with_retry()?;
    let serialized = serde_json::to_string(&request)?;
    pipe.write_all(serialized.as_bytes())?;
    pipe.write_all(b"\n")?;
    pipe.flush()?;

    let mut reader = BufReader::new(pipe);
    let mut line = String::new();
    reader.read_line(&mut line)?;

    if line.trim().is_empty() {
        println!("<empty>");
    } else {
        println!("{line}");
    }

    Ok(())
}

fn open_pipe_with_retry() -> Result<std::fs::File, Box<dyn std::error::Error + Send + Sync>> {
    let mut last_error: Option<std::io::Error> = None;

    for _ in 0..20 {
        match OpenOptions::new().read(true).write(true).open(PIPE_PATH) {
            Ok(pipe) => return Ok(pipe),
            Err(error) => {
                last_error = Some(error);
                thread::sleep(Duration::from_millis(100));
            }
        }
    }

    Err(last_error
        .unwrap_or_else(|| std::io::Error::other("Failed to open named pipe"))
        .into())
}
