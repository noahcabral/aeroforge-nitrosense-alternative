use std::{
    fs, io,
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use futures_util::{SinkExt, StreamExt};
use native_tls::TlsConnector as NativeTlsConnector;
use serde_json::{json, Map, Value};
use tokio::net::TcpStream;
use tokio_tungstenite::{
    connect_async_tls_with_config, tungstenite::client::IntoClientRequest, tungstenite::Message,
    Connector, MaybeTlsStream, WebSocketStream,
};

type DynError = Box<dyn std::error::Error + Send + Sync>;

const CARE_CENTER_URL: &str = "wss://127.0.0.1:4343/";
const CARE_CENTER_AUTH_DATA: &str = "OO24T5iPpUKxfxjPZW4ddo3uGeIJXC+ZbJPzCNGMBenvnZ7J9xQjpykAdgAL1a9HocmZsOHxisitF9B0t7msCayFnVDZA79rADTRjUNWQaLEQA6wSpLrLo/Fu/gHH7BM";
const SETTINGS_PATH: &str = r"C:\ProgramData\Acer\CC\settings.json";
const CARE_CENTER_TIMEOUT: Duration = Duration::from_secs(5);

pub struct SmartChargeApplyPayload {
    pub enabled: bool,
    pub battery_healthy: u8,
    pub applied_at_unix: u64,
    pub detail: String,
}

pub async fn sync_saved_state(enabled: bool) -> Result<SmartChargeApplyPayload, DynError> {
    apply_smart_charging(enabled).await
}

pub async fn apply_smart_charging(enabled: bool) -> Result<SmartChargeApplyPayload, DynError> {
    let requested_battery_healthy = if enabled { 0 } else { 1 };
    let mut client =
        with_timeout("connect to Acer Care Center", CareCenterClient::connect()).await?;

    with_timeout(
        "set Acer Care Center BatteryHealthy",
        client.set_battery_healthy(requested_battery_healthy),
    )
    .await?;
    let verified_battery_healthy = with_timeout(
        "read Acer Care Center BatteryHealthy",
        client.get_battery_healthy(),
    )
    .await?;
    if verified_battery_healthy != requested_battery_healthy {
        return Err(io::Error::other(format!(
            "Care Center returned BatteryHealthy {} after requesting {}.",
            verified_battery_healthy, requested_battery_healthy
        ))
        .into());
    }

    let boundary = with_timeout(
        "read Acer Care Center BatteryBoundary",
        client.get_battery_boundary(),
    )
    .await
    .ok();
    persist_battery_healthy(verified_battery_healthy)?;

    let detail = match (enabled, boundary) {
        (true, Some(boundary)) => format!(
            "Applied Acer Care Center optimized charging (BatteryHealthy=0). BatteryBoundary reports {}-{} with the 80% ceiling active.",
            boundary.lower_bound, boundary.upper_bound
        ),
        (true, None) => {
            "Applied Acer Care Center optimized charging (BatteryHealthy=0) with the 80% ceiling active."
                .into()
        }
        (false, Some(boundary)) => format!(
            "Applied full battery charging through Acer Care Center (BatteryHealthy=1). BatteryBoundary still reports {}-{}, but BatteryHealthy is the real full-charge gate on this SKU.",
            boundary.lower_bound, boundary.upper_bound
        ),
        (false, None) => {
            "Applied full battery charging through Acer Care Center (BatteryHealthy=1)."
                .into()
        }
    };

    Ok(SmartChargeApplyPayload {
        enabled,
        battery_healthy: verified_battery_healthy,
        applied_at_unix: now_unix(),
        detail,
    })
}

async fn with_timeout<F, T>(label: &str, future: F) -> Result<T, DynError>
where
    F: std::future::Future<Output = Result<T, DynError>>,
{
    match tokio::time::timeout(CARE_CENTER_TIMEOUT, future).await {
        Ok(result) => result,
        Err(_) => Err(io::Error::other(format!("Timed out while trying to {label}.")).into()),
    }
}

struct CareCenterClient {
    socket: WebSocketStream<MaybeTlsStream<TcpStream>>,
}

impl CareCenterClient {
    async fn connect() -> Result<Self, DynError> {
        let mut request = CARE_CENTER_URL.into_client_request()?;
        request
            .headers_mut()
            .insert("Origin", "https://127.0.0.1".parse()?);

        let tls = NativeTlsConnector::builder()
            .danger_accept_invalid_certs(true)
            .danger_accept_invalid_hostnames(true)
            .build()?;
        let connector = Connector::NativeTls(tls);
        let (mut socket, _) =
            connect_async_tls_with_config(request, None, false, Some(connector)).await?;

        socket
            .send(Message::Text(
                json!({
                    "PacketType": 1,
                    "Version": 1,
                    "Data": CARE_CENTER_AUTH_DATA,
                })
                .to_string(),
            ))
            .await?;
        let auth_reply = recv_json(&mut socket).await?;
        let packet_type = auth_reply
            .get("PacketType")
            .and_then(Value::as_u64)
            .unwrap_or_default();
        if packet_type != 1 {
            return Err(io::Error::other(format!(
                "Unexpected Care Center auth response: {auth_reply}"
            ))
            .into());
        }

        socket
            .send(Message::Text(
                json!({
                    "PacketType": 2,
                    "Version": 1,
                    "Session": "af-function-query",
                    "Command": "FunctionQuery",
                })
                .to_string(),
            ))
            .await?;
        let function_query = recv_json(&mut socket).await?;
        let battery_healthy_supported = function_query
            .get("Data")
            .and_then(Value::as_array)
            .map(|items| items.iter().any(|entry| entry == "BatteryHealthy"))
            .unwrap_or(false);
        if !battery_healthy_supported {
            return Err(io::Error::other(
                "Acer Care Center does not report BatteryHealthy support on this machine.",
            )
            .into());
        }

        Ok(Self { socket })
    }

    async fn set_battery_healthy(&mut self, value: u8) -> Result<(), DynError> {
        let response = self
            .send_command(json!({
                "PacketType": 2,
                "Version": 1,
                "Session": format!("af-bh-set-{value}"),
                "Command": "BatteryHealthy",
                "Action": "Set",
                "Param1": value,
            }))
            .await?;

        let packet_type = response
            .get("PacketType")
            .and_then(Value::as_u64)
            .unwrap_or_default();
        if packet_type != 3 {
            return Err(io::Error::other(format!(
                "Unexpected BatteryHealthy set response: {response}"
            ))
            .into());
        }
        Ok(())
    }

    async fn get_battery_healthy(&mut self) -> Result<u8, DynError> {
        let response = self
            .send_command(json!({
                "PacketType": 2,
                "Version": 1,
                "Session": "af-bh-get",
                "Command": "BatteryHealthy",
                "Action": "Get",
            }))
            .await?;
        response
            .get("Result")
            .and_then(Value::as_object)
            .and_then(|result| result.get("Value"))
            .and_then(Value::as_u64)
            .and_then(|value| u8::try_from(value).ok())
            .ok_or_else(|| {
                io::Error::other(format!(
                    "BatteryHealthy get response did not contain Value: {response}"
                ))
                .into()
            })
    }

    async fn get_battery_boundary(&mut self) -> Result<BatteryBoundary, DynError> {
        let response = self
            .send_command(json!({
                "PacketType": 2,
                "Version": 1,
                "Session": "af-boundary-get",
                "Command": "BatteryBoundary",
                "Action": "Get",
            }))
            .await?;
        let result = response
            .get("Result")
            .and_then(Value::as_object)
            .ok_or_else(|| {
                io::Error::other(format!(
                    "BatteryBoundary response missing Result object: {response}"
                ))
            })?;
        let upper_bound = result
            .get("UpperBound")
            .and_then(Value::as_u64)
            .and_then(|value| u8::try_from(value).ok())
            .ok_or_else(|| {
                io::Error::other(format!(
                    "BatteryBoundary response missing UpperBound: {response}"
                ))
            })?;
        let lower_bound = result
            .get("LowerBound")
            .and_then(Value::as_u64)
            .and_then(|value| u8::try_from(value).ok())
            .ok_or_else(|| {
                io::Error::other(format!(
                    "BatteryBoundary response missing LowerBound: {response}"
                ))
            })?;

        Ok(BatteryBoundary {
            upper_bound,
            lower_bound,
        })
    }

    async fn send_command(&mut self, payload: Value) -> Result<Value, DynError> {
        self.socket.send(Message::Text(payload.to_string())).await?;
        recv_json(&mut self.socket).await
    }
}

struct BatteryBoundary {
    upper_bound: u8,
    lower_bound: u8,
}

async fn recv_json(
    socket: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
) -> Result<Value, DynError> {
    while let Some(message) = socket.next().await {
        match message? {
            Message::Text(text) => return Ok(serde_json::from_str(&text)?),
            Message::Binary(bytes) => return Ok(serde_json::from_slice(&bytes)?),
            Message::Ping(payload) => socket.send(Message::Pong(payload)).await?,
            Message::Pong(_) => {}
            Message::Frame(_) => {}
            Message::Close(frame) => {
                return Err(io::Error::other(format!(
                    "Care Center websocket closed before a JSON reply arrived: {frame:?}"
                ))
                .into())
            }
        }
    }

    Err(io::Error::other("Care Center websocket ended before a JSON reply arrived.").into())
}

fn persist_battery_healthy(battery_healthy: u8) -> Result<(), DynError> {
    let path = Path::new(SETTINGS_PATH);
    let mut root = if path.exists() {
        serde_json::from_str::<Value>(&fs::read_to_string(path)?)?
    } else {
        Value::Object(Map::new())
    };

    let object = root
        .as_object_mut()
        .ok_or_else(|| io::Error::other("Care Center settings.json was not a JSON object."))?;
    object.insert("BatteryHealthy".into(), Value::from(battery_healthy));

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    fs::write(path, serde_json::to_string_pretty(&root)?)?;
    Ok(())
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
