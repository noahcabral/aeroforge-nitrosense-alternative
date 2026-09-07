use libloading::Library;
use std::ffi::c_void;

const NVAPI_OK: i32 = 0;
const NVAPI_INIT_CANDIDATE_ID: u32 = 0x0150_E828;
const NVAPI_WHISPER_SETTER_ID: u32 = 0xD256_1B69;
const WHISPER_PAYLOAD_SIZE: usize = 0x4C8;

type NvApiQueryInterface = unsafe extern "C" fn(u32) -> *mut c_void;
type NvApiInitializeCandidate = unsafe extern "C" fn() -> i32;
type NvApiWhisperSetter = unsafe extern "C" fn(*mut c_void, *mut c_void, isize, *mut c_void) -> i32;

#[derive(Debug, Clone)]
pub struct NvApiWhisperResult {
    pub enabled: bool,
    pub init_candidate_id: u32,
    pub init_status: i32,
    pub hidden_id: u32,
    pub status: i32,
}

pub fn set_whisper_mode(
    enable: bool,
) -> Result<NvApiWhisperResult, Box<dyn std::error::Error + Send + Sync>> {
    let library = unsafe { Library::new("nvapi64.dll") }?;
    let query = unsafe {
        *library
            .get::<NvApiQueryInterface>(b"nvapi_QueryInterface\0")
            .map_err(|error| format!("Failed to resolve nvapi_QueryInterface: {error}"))?
    };

    let init_pointer = unsafe { query(NVAPI_INIT_CANDIDATE_ID) };
    if init_pointer.is_null() {
        return Err(format!(
            "nvapi_QueryInterface(0x{:08X}) returned null for the standalone init candidate.",
            NVAPI_INIT_CANDIDATE_ID
        )
        .into());
    }

    let init_candidate: NvApiInitializeCandidate = unsafe { std::mem::transmute(init_pointer) };
    let init_status = unsafe { init_candidate() };
    if init_status != NVAPI_OK {
        return Err(format!(
            "NVAPI init candidate 0x{:08X} failed with status {}.",
            NVAPI_INIT_CANDIDATE_ID, init_status
        )
        .into());
    }

    let setter_pointer = unsafe { query(NVAPI_WHISPER_SETTER_ID) };
    if setter_pointer.is_null() {
        return Err(format!(
            "nvapi_QueryInterface(0x{:08X}) returned null for the Whisper setter.",
            NVAPI_WHISPER_SETTER_ID
        )
        .into());
    }

    let setter: NvApiWhisperSetter = unsafe { std::mem::transmute(setter_pointer) };
    let mut payload = [0u8; WHISPER_PAYLOAD_SIZE];
    payload[0x00] = 0xC8;
    payload[0x01] = 0x04;
    payload[0x02] = 0x01;
    payload[0x04] = 0x01;
    payload[0x4C] = if enable { 0x01 } else { 0x00 };

    let status = unsafe {
        setter(
            payload.as_mut_ptr().cast(),
            std::ptr::null_mut(),
            WHISPER_PAYLOAD_SIZE as isize,
            std::ptr::null_mut(),
        )
    };
    if status != NVAPI_OK {
        return Err(format!(
            "NVAPI Whisper setter 0x{:08X} failed with status {}.",
            NVAPI_WHISPER_SETTER_ID, status
        )
        .into());
    }

    Ok(NvApiWhisperResult {
        enabled: enable,
        init_candidate_id: NVAPI_INIT_CANDIDATE_ID,
        init_status,
        hidden_id: NVAPI_WHISPER_SETTER_ID,
        status,
    })
}
