use std::{ffi::c_void, ptr::null_mut};

const S_OK: i32 = 0;
const S_FALSE: i32 = 1;
const RPC_E_CHANGED_MODE: i32 = 0x8001_0106u32 as i32;
const RPC_E_TOO_LATE: i32 = 0x8001_0119u32 as i32;

const COINIT_MULTITHREADED: u32 = 0;
const CLSCTX_INPROC_SERVER: u32 = 0x1;
const RPC_C_AUTHN_LEVEL_DEFAULT: u32 = 0;
const RPC_C_IMP_LEVEL_IMPERSONATE: u32 = 3;
const EOAC_NONE: u32 = 0;

const VT_EMPTY: u16 = 0;
const VT_NULL: u16 = 1;
const VT_I2: u16 = 2;
const VT_I4: u16 = 3;
const VT_BSTR: u16 = 8;
const VT_UI1: u16 = 17;
const VT_UI2: u16 = 18;
const VT_UI4: u16 = 19;
const VT_I8: u16 = 20;
const VT_UI8: u16 = 21;
const CIM_UINT32: i32 = 19;
const CIM_UINT64: i32 = 21;
const WBEM_E_NOT_FOUND: i32 = 0x8004_1002u32 as i32;

const CLSID_WBEM_LOCATOR: Guid = Guid::new(
    0x4590_f811,
    0x1d3a,
    0x11d0,
    [0x89, 0x1f, 0x00, 0xaa, 0x00, 0x4b, 0x2e, 0x24],
);
const IID_IWBEM_LOCATOR: Guid = Guid::new(
    0xdc12_a687,
    0x737f,
    0x11cf,
    [0x88, 0x4d, 0x00, 0xaa, 0x00, 0x4b, 0x2e, 0x24],
);

const ACER_WMI_NAMESPACE: &str = "ROOT\\WMI";
const ACER_GAMING_CLASS: &str = "AcerGamingFunction";
const ACER_GAMING_OBJECT_PATH: &str =
    "AcerGamingFunction.InstanceName=\"ACPI\\\\PNP0C14\\\\APGe_0\"";
const GM_INPUT: &str = "gmInput";
const GM_OUTPUT: &str = "gmOutput";

pub const GAMING_PROFILE_BALANCED: u64 = 0x0000_0001;
pub const GAMING_PROFILE_PERFORMANCE: u64 = 0x0000_0004;
pub const GAMING_PROFILE_TURBO: u64 = 0x0000_0005;
pub const GAMING_PROFILE_QUIET: u8 = 0x00;

pub const FAN_BEHAVIOR_AUTO: u64 = 0x0041_0009;
pub const FAN_BEHAVIOR_MAX: u64 = 0x0082_0009;
pub const FAN_BEHAVIOR_CUSTOM_MIXED: u64 = 0x00C3_0009;

pub const FAN_SELECTOR_CPU: u8 = 0x01;
pub const FAN_SELECTOR_GPU: u8 = 0x04;
pub const MIN_MANUAL_FAN_PERCENT: u8 = 2;

pub const MISC_SETTING_SUPPORTED_PROFILES: u8 = 0x0A;
pub const MISC_SETTING_PLATFORM_PROFILE: u8 = 0x0B;

pub fn clamp_manual_fan_percent(percent: u8) -> u8 {
    percent.clamp(MIN_MANUAL_FAN_PERCENT, 100)
}

pub fn decode_gm_output_byte(value: u64) -> u8 {
    let shifted = ((value >> 8) & 0xFF) as u8;
    if shifted != 0 || value > 0xFF {
        shifted
    } else {
        (value & 0xFF) as u8
    }
}

pub fn apply_gaming_profile(
    input: u64,
) -> Result<AcerWmiMethodResult, Box<dyn std::error::Error + Send + Sync>> {
    invoke_acer_gaming_u64_method("SetGamingProfile", input)
}

pub fn apply_gaming_misc_setting(
    setting: u8,
    value: u8,
) -> Result<AcerWmiMethodResult, Box<dyn std::error::Error + Send + Sync>> {
    let input = u64::from(setting) | (u64::from(value) << 8);
    invoke_acer_gaming_u64_method("SetGamingMiscSetting", input)
}

pub fn read_gaming_misc_setting(
    setting: u8,
) -> Result<AcerWmiMethodResult, Box<dyn std::error::Error + Send + Sync>> {
    invoke_acer_gaming_u64_method("GetGamingMiscSetting", u64::from(setting))
}

pub fn apply_fan_behavior(
    input: u64,
) -> Result<AcerWmiMethodResult, Box<dyn std::error::Error + Send + Sync>> {
    invoke_acer_gaming_u64_method("SetGamingFanBehavior", input)
}

pub fn apply_fan_speed(
    selector: u8,
    percent: u8,
) -> Result<AcerWmiMethodResult, Box<dyn std::error::Error + Send + Sync>> {
    let clamped = clamp_manual_fan_percent(percent);
    let input = (u64::from(clamped) << 8) | u64::from(selector);
    invoke_acer_gaming_u64_method("SetGamingFanSpeed", input)
}

pub fn read_firmware_sensor_snapshot(
) -> Result<AcerFirmwareSensorReadback, Box<dyn std::error::Error + Send + Sync>> {
    let mut errors = Vec::new();
    let supported_sensors = read_sysinfo_raw(0x0000, &mut errors);
    let battery_status = read_sysinfo_raw(0x0002, &mut errors);
    let cpu_temp = read_sysinfo_raw(0x0101, &mut errors);
    let cpu_fan = read_sysinfo_raw(0x0201, &mut errors);
    let system_temp = read_sysinfo_raw(0x0301, &mut errors);
    let gpu_fan = read_sysinfo_raw(0x0601, &mut errors);
    let gpu_temp = read_sysinfo_raw(0x0A01, &mut errors);

    if [
        supported_sensors,
        battery_status,
        cpu_temp,
        cpu_fan,
        system_temp,
        gpu_fan,
        gpu_temp,
    ]
    .iter()
    .all(Option::is_none)
    {
        return Err(format!(
            "AcerGamingFunction GetGamingSysInfo did not return any firmware sensor values. {}",
            errors.join(" | ")
        )
        .into());
    }

    Ok(AcerFirmwareSensorReadback {
        supported_sensors: supported_sensors.map(|value| ((value >> 24) & 0xFFFF) as u16),
        battery_status,
        cpu_temp_c: decode_sysinfo_sensor(cpu_temp),
        gpu_temp_c: decode_sysinfo_sensor(gpu_temp),
        system_temp_c: decode_sysinfo_sensor(system_temp),
        cpu_fan_rpm: decode_sysinfo_sensor(cpu_fan),
        gpu_fan_rpm: decode_sysinfo_sensor(gpu_fan),
    })
}

pub fn read_gaming_sys_info(
    input: u32,
) -> Result<AcerWmiMethodResult, Box<dyn std::error::Error + Send + Sync>> {
    invoke_acer_gaming_u64_method("GetGamingSysInfo", u64::from(input))
}

#[derive(Debug, Clone)]
pub struct AcerWmiMethodResult {
    pub method: &'static str,
    pub input: u64,
    pub hresult: i32,
    pub output: Option<u64>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AcerFirmwareSensorReadback {
    pub supported_sensors: Option<u16>,
    pub battery_status: Option<u64>,
    pub cpu_temp_c: Option<u16>,
    pub gpu_temp_c: Option<u16>,
    pub system_temp_c: Option<u16>,
    pub cpu_fan_rpm: Option<u16>,
    pub gpu_fan_rpm: Option<u16>,
}

struct ComApartment {
    should_uninitialize: bool,
}

impl ComApartment {
    fn initialize() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let hr = unsafe { CoInitializeEx(null_mut(), COINIT_MULTITHREADED) };
        if !hresult_ok(hr) && hr != RPC_E_CHANGED_MODE {
            return Err(format!("CoInitializeEx failed with 0x{:08X}", hr as u32).into());
        }

        let security_hr = unsafe {
            CoInitializeSecurity(
                null_mut(),
                -1,
                null_mut(),
                null_mut(),
                RPC_C_AUTHN_LEVEL_DEFAULT,
                RPC_C_IMP_LEVEL_IMPERSONATE,
                null_mut(),
                EOAC_NONE,
                null_mut(),
            )
        };
        if !hresult_ok(security_hr) && security_hr != RPC_E_TOO_LATE {
            return Err(format!(
                "CoInitializeSecurity failed with 0x{:08X}",
                security_hr as u32
            )
            .into());
        }

        Ok(Self {
            should_uninitialize: hr == S_OK || hr == S_FALSE,
        })
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        if self.should_uninitialize {
            unsafe {
                CoUninitialize();
            }
        }
    }
}

fn invoke_acer_gaming_u64_method(
    method: &'static str,
    input: u64,
) -> Result<AcerWmiMethodResult, Box<dyn std::error::Error + Send + Sync>> {
    let _com = ComApartment::initialize()?;

    let locator = WbemLocator::create()?;
    let services = locator.connect(ACER_WMI_NAMESPACE)?;
    let class_object = services.get_object(ACER_GAMING_CLASS)?;
    let input_signature = class_object.get_method_input_signature(method)?;
    let input_instance = input_signature.spawn_instance()?;
    input_instance.put_u64(GM_INPUT, input)?;

    let execution =
        services.exec_method(ACER_GAMING_OBJECT_PATH, method, input_instance.as_ptr())?;
    let output = execution
        .output
        .as_ref()
        .map(|params| params.try_get_u64(GM_OUTPUT))
        .transpose()?
        .flatten();

    Ok(AcerWmiMethodResult {
        method,
        input,
        hresult: execution.hresult,
        output,
    })
}

fn read_sysinfo_raw(input: u32, errors: &mut Vec<String>) -> Option<u64> {
    match read_gaming_sys_info(input) {
        Ok(result) => result.output,
        Err(error) => {
            errors.push(format!("0x{input:X}: {error}"));
            None
        }
    }
}

fn decode_sysinfo_sensor(value: Option<u64>) -> Option<u16> {
    value.map(|value| ((value >> 8) & 0xFFFF) as u16)
}

struct BStr(*mut u16);

impl BStr {
    fn new(value: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let wide: Vec<u16> = value.encode_utf16().collect();
        let ptr = unsafe { SysAllocStringLen(wide.as_ptr(), wide.len() as u32) };
        if ptr.is_null() {
            return Err(format!("SysAllocStringLen failed for {value}").into());
        }
        Ok(Self(ptr))
    }

    fn as_ptr(&self) -> *mut u16 {
        self.0
    }
}

impl Drop for BStr {
    fn drop(&mut self) {
        unsafe {
            SysFreeString(self.0);
        }
    }
}

struct WbemLocator(*mut IWbemLocator);

impl WbemLocator {
    fn create() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let mut locator: *mut IWbemLocator = null_mut();
        let hr = unsafe {
            CoCreateInstance(
                &CLSID_WBEM_LOCATOR,
                null_mut(),
                CLSCTX_INPROC_SERVER,
                &IID_IWBEM_LOCATOR,
                &mut locator as *mut _ as *mut *mut c_void,
            )
        };
        hresult(hr, "CoCreateInstance(CLSID_WbemLocator)")?;
        if locator.is_null() {
            return Err("CoCreateInstance(CLSID_WbemLocator) returned null.".into());
        }
        Ok(Self(locator))
    }

    fn connect(
        &self,
        namespace: &str,
    ) -> Result<WbemServices, Box<dyn std::error::Error + Send + Sync>> {
        let namespace = BStr::new(namespace)?;
        let mut services: *mut IWbemServices = null_mut();
        let hr = unsafe {
            ((*(*self.0).vtable).connect_server)(
                self.0,
                namespace.as_ptr(),
                null_mut(),
                null_mut(),
                null_mut(),
                0,
                null_mut(),
                null_mut(),
                &mut services,
            )
        };
        hresult(hr, "IWbemLocator::ConnectServer(ROOT\\WMI)")?;
        if services.is_null() {
            return Err("IWbemLocator::ConnectServer returned null services.".into());
        }

        let proxy_hr = unsafe {
            CoSetProxyBlanket(
                services as *mut c_void,
                10,
                0,
                null_mut(),
                3,
                RPC_C_IMP_LEVEL_IMPERSONATE,
                null_mut(),
                EOAC_NONE,
            )
        };
        hresult(proxy_hr, "CoSetProxyBlanket(IWbemServices)")?;

        Ok(WbemServices(services))
    }
}

impl Drop for WbemLocator {
    fn drop(&mut self) {
        unsafe {
            ((*(*self.0).vtable).release)(self.0);
        }
    }
}

struct WbemServices(*mut IWbemServices);

impl WbemServices {
    fn get_object(
        &self,
        object_path: &str,
    ) -> Result<WbemClassObject, Box<dyn std::error::Error + Send + Sync>> {
        let path = BStr::new(object_path)?;
        let mut object: *mut IWbemClassObject = null_mut();
        let hr = unsafe {
            ((*(*self.0).vtable).get_object)(
                self.0,
                path.as_ptr(),
                0,
                null_mut(),
                &mut object,
                null_mut(),
            )
        };
        hresult(hr, "IWbemServices::GetObject(AcerGamingFunction)")?;
        if object.is_null() {
            return Err("IWbemServices::GetObject returned null object.".into());
        }
        Ok(WbemClassObject(object))
    }

    fn exec_method(
        &self,
        object_path: &str,
        method: &str,
        input_params: *mut IWbemClassObject,
    ) -> Result<ExecMethodResult, Box<dyn std::error::Error + Send + Sync>> {
        let object_path = BStr::new(object_path)?;
        let method = BStr::new(method)?;
        let mut output_params: *mut IWbemClassObject = null_mut();
        let hr = unsafe {
            ((*(*self.0).vtable).exec_method)(
                self.0,
                object_path.as_ptr(),
                method.as_ptr(),
                0,
                null_mut(),
                input_params,
                &mut output_params,
                null_mut(),
            )
        };
        hresult(hr, "IWbemServices::ExecMethod")?;

        Ok(ExecMethodResult {
            hresult: hr,
            output: if output_params.is_null() {
                None
            } else {
                Some(WbemClassObject(output_params))
            },
        })
    }
}

impl Drop for WbemServices {
    fn drop(&mut self) {
        unsafe {
            ((*(*self.0).vtable).release)(self.0);
        }
    }
}

struct WbemClassObject(*mut IWbemClassObject);

impl WbemClassObject {
    fn as_ptr(&self) -> *mut IWbemClassObject {
        self.0
    }

    fn get_method_input_signature(
        &self,
        method: &str,
    ) -> Result<WbemClassObject, Box<dyn std::error::Error + Send + Sync>> {
        let method = BStr::new(method)?;
        let mut input_signature: *mut IWbemClassObject = null_mut();
        let hr = unsafe {
            ((*(*self.0).vtable).get_method)(
                self.0,
                method.as_ptr(),
                0,
                &mut input_signature,
                null_mut(),
            )
        };
        hresult(hr, "IWbemClassObject::GetMethod")?;
        if input_signature.is_null() {
            return Err("IWbemClassObject::GetMethod returned null input signature.".into());
        }
        Ok(WbemClassObject(input_signature))
    }

    fn spawn_instance(&self) -> Result<WbemClassObject, Box<dyn std::error::Error + Send + Sync>> {
        let mut instance: *mut IWbemClassObject = null_mut();
        let hr = unsafe { ((*(*self.0).vtable).spawn_instance)(self.0, 0, &mut instance) };
        hresult(hr, "IWbemClassObject::SpawnInstance")?;
        if instance.is_null() {
            return Err("IWbemClassObject::SpawnInstance returned null instance.".into());
        }
        Ok(WbemClassObject(instance))
    }

    fn put_u64(
        &self,
        name: &str,
        value: u64,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let name = BStr::new(name)?;
        if let Ok(value32) = u32::try_from(value) {
            let mut variant = Variant::u32(value32);
            let hr = unsafe {
                ((*(*self.0).vtable).put)(
                    self.0,
                    name.as_ptr(),
                    0,
                    variant.as_mut_ptr(),
                    CIM_UINT32,
                )
            };
            unsafe {
                VariantClear(variant.as_mut_ptr());
            }
            if hresult_ok(hr) {
                return Ok(());
            }

            let mut fallback = Variant::wmi_u64(value)?;
            let fallback_hr = unsafe {
                ((*(*self.0).vtable).put)(
                    self.0,
                    name.as_ptr(),
                    0,
                    fallback.as_mut_ptr(),
                    CIM_UINT64,
                )
            };
            unsafe {
                VariantClear(fallback.as_mut_ptr());
            }
            if hresult_ok(fallback_hr) {
                return Ok(());
            }

            return Err(format!(
                "IWbemClassObject::Put(gmInput) failed with UInt32 0x{:08X} and UInt64 fallback 0x{:08X}",
                hr as u32, fallback_hr as u32
            )
            .into());
        }

        let mut variant = Variant::wmi_u64(value)?;
        let hr = unsafe {
            ((*(*self.0).vtable).put)(self.0, name.as_ptr(), 0, variant.as_mut_ptr(), CIM_UINT64)
        };
        unsafe {
            VariantClear(variant.as_mut_ptr());
        }
        hresult(hr, "IWbemClassObject::Put(gmInput)")?;
        Ok(())
    }

    fn try_get_u64(
        &self,
        name: &str,
    ) -> Result<Option<u64>, Box<dyn std::error::Error + Send + Sync>> {
        let name = BStr::new(name)?;
        let mut variant = Variant::empty();
        let mut cim_type = 0i32;
        let mut flavor = 0i32;
        let hr = unsafe {
            ((*(*self.0).vtable).get)(
                self.0,
                name.as_ptr(),
                0,
                variant.as_mut_ptr(),
                &mut cim_type,
                &mut flavor,
            )
        };
        if hr == WBEM_E_NOT_FOUND {
            return Ok(None);
        }

        hresult(hr, "IWbemClassObject::Get(gmOutput)")?;
        let parsed = variant.to_u64();
        unsafe {
            VariantClear(variant.as_mut_ptr());
        }
        parsed
    }
}

impl Drop for WbemClassObject {
    fn drop(&mut self) {
        unsafe {
            ((*(*self.0).vtable).release)(self.0);
        }
    }
}

fn hresult_ok(hr: i32) -> bool {
    hr >= 0
}

fn hresult(hr: i32, context: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if hresult_ok(hr) {
        Ok(())
    } else {
        Err(format!("{context} failed with 0x{:08X}", hr as u32).into())
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Guid {
    data1: u32,
    data2: u16,
    data3: u16,
    data4: [u8; 8],
}

impl Guid {
    const fn new(data1: u32, data2: u16, data3: u16, data4: [u8; 8]) -> Self {
        Self {
            data1,
            data2,
            data3,
            data4,
        }
    }
}

#[repr(C)]
struct Variant {
    vt: u16,
    reserved1: u16,
    reserved2: u16,
    reserved3: u16,
    data: VariantData,
}

impl Variant {
    fn empty() -> Self {
        Self {
            vt: VT_EMPTY,
            reserved1: 0,
            reserved2: 0,
            reserved3: 0,
            data: VariantData { reserved: 0 },
        }
    }

    fn wmi_u64(value: u64) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let text = value.to_string();
        let wide: Vec<u16> = text.encode_utf16().collect();
        let ptr = unsafe { SysAllocStringLen(wide.as_ptr(), wide.len() as u32) };
        if ptr.is_null() {
            return Err(format!("SysAllocStringLen failed for WMI UInt64 value {value}").into());
        }

        Ok(Self {
            vt: VT_BSTR,
            reserved1: 0,
            reserved2: 0,
            reserved3: 0,
            data: VariantData { bstr_val: ptr },
        })
    }

    fn u32(value: u32) -> Self {
        Self {
            vt: VT_UI4,
            reserved1: 0,
            reserved2: 0,
            reserved3: 0,
            data: VariantData { ul_val: value },
        }
    }

    fn as_mut_ptr(&mut self) -> *mut Self {
        self
    }

    fn to_u64(&self) -> Result<Option<u64>, Box<dyn std::error::Error + Send + Sync>> {
        let value = match self.vt {
            VT_EMPTY | VT_NULL => None,
            VT_BSTR => {
                let text = unsafe { bstr_to_string(self.data.bstr_val) };
                if text.trim().is_empty() {
                    None
                } else {
                    Some(text.trim().parse::<u64>()?)
                }
            }
            VT_UI1 => Some(unsafe { self.data.b_val } as u64),
            VT_UI2 => Some(unsafe { self.data.ui_val } as u64),
            VT_UI4 => Some(unsafe { self.data.ul_val } as u64),
            VT_UI8 => Some(unsafe { self.data.ull_val }),
            VT_I2 => Some(u64::try_from(unsafe { self.data.i_val })?),
            VT_I4 => Some(u64::try_from(unsafe { self.data.l_val })?),
            VT_I8 => Some(u64::try_from(unsafe { self.data.ll_val })?),
            other => {
                return Err(
                    format!("Unsupported VARIANT type {other} for WMI UInt64 output.").into(),
                )
            }
        };

        Ok(value)
    }
}

#[repr(C)]
union VariantData {
    bstr_val: *mut u16,
    ull_val: u64,
    ll_val: i64,
    ul_val: u32,
    l_val: i32,
    ui_val: u16,
    i_val: i16,
    b_val: u8,
    reserved: u64,
}

struct ExecMethodResult {
    hresult: i32,
    output: Option<WbemClassObject>,
}

unsafe fn bstr_to_string(value: *mut u16) -> String {
    if value.is_null() {
        return String::new();
    }

    let len = SysStringLen(value) as usize;
    String::from_utf16_lossy(std::slice::from_raw_parts(value, len))
}

#[repr(C)]
struct IWbemLocator {
    vtable: *const IWbemLocatorVtbl,
}

#[repr(C)]
struct IWbemLocatorVtbl {
    query_interface: usize,
    add_ref: usize,
    release: unsafe extern "system" fn(*mut IWbemLocator) -> u32,
    connect_server: unsafe extern "system" fn(
        *mut IWbemLocator,
        *mut u16,
        *mut u16,
        *mut u16,
        *mut u16,
        i32,
        *mut u16,
        *mut c_void,
        *mut *mut IWbemServices,
    ) -> i32,
}

#[repr(C)]
struct IWbemServices {
    vtable: *const IWbemServicesVtbl,
}

#[repr(C)]
struct IWbemServicesVtbl {
    query_interface: usize,
    add_ref: usize,
    release: unsafe extern "system" fn(*mut IWbemServices) -> u32,
    open_namespace: usize,
    cancel_async_call: usize,
    query_object_sink: usize,
    get_object: unsafe extern "system" fn(
        *mut IWbemServices,
        *mut u16,
        i32,
        *mut c_void,
        *mut *mut IWbemClassObject,
        *mut c_void,
    ) -> i32,
    get_object_async: usize,
    put_class: usize,
    put_class_async: usize,
    delete_class: usize,
    delete_class_async: usize,
    create_class_enum: usize,
    create_class_enum_async: usize,
    put_instance: usize,
    put_instance_async: usize,
    delete_instance: usize,
    delete_instance_async: usize,
    create_instance_enum: usize,
    create_instance_enum_async: usize,
    exec_query: usize,
    exec_query_async: usize,
    exec_notification_query: usize,
    exec_notification_query_async: usize,
    exec_method: unsafe extern "system" fn(
        *mut IWbemServices,
        *mut u16,
        *mut u16,
        i32,
        *mut c_void,
        *mut IWbemClassObject,
        *mut *mut IWbemClassObject,
        *mut c_void,
    ) -> i32,
    exec_method_async: usize,
}

#[repr(C)]
struct IWbemClassObject {
    vtable: *const IWbemClassObjectVtbl,
}

#[repr(C)]
struct IWbemClassObjectVtbl {
    query_interface: usize,
    add_ref: usize,
    release: unsafe extern "system" fn(*mut IWbemClassObject) -> u32,
    get_qualifier_set: usize,
    get: unsafe extern "system" fn(
        *mut IWbemClassObject,
        *mut u16,
        i32,
        *mut Variant,
        *mut i32,
        *mut i32,
    ) -> i32,
    put: unsafe extern "system" fn(*mut IWbemClassObject, *mut u16, i32, *mut Variant, i32) -> i32,
    delete: usize,
    get_names: usize,
    begin_enumeration: usize,
    next: usize,
    end_enumeration: usize,
    get_property_qualifier_set: usize,
    clone: usize,
    get_object_text: usize,
    spawn_derived_class: usize,
    spawn_instance:
        unsafe extern "system" fn(*mut IWbemClassObject, i32, *mut *mut IWbemClassObject) -> i32,
    compare_to: usize,
    get_property_origin: usize,
    inherits_from: usize,
    get_method: unsafe extern "system" fn(
        *mut IWbemClassObject,
        *mut u16,
        i32,
        *mut *mut IWbemClassObject,
        *mut *mut IWbemClassObject,
    ) -> i32,
}

#[link(name = "ole32")]
extern "system" {
    fn CoInitializeEx(reserved: *mut c_void, coinit: u32) -> i32;
    fn CoUninitialize();
    fn CoInitializeSecurity(
        security_descriptor: *mut c_void,
        auth_service_count: i32,
        auth_services: *mut c_void,
        reserved1: *mut c_void,
        authn_level: u32,
        imp_level: u32,
        auth_list: *mut c_void,
        capabilities: u32,
        reserved3: *mut c_void,
    ) -> i32;
    fn CoCreateInstance(
        clsid: *const Guid,
        outer: *mut c_void,
        context: u32,
        iid: *const Guid,
        object: *mut *mut c_void,
    ) -> i32;
    fn CoSetProxyBlanket(
        proxy: *mut c_void,
        authn_service: u32,
        authz_service: u32,
        server_principal_name: *mut u16,
        authn_level: u32,
        imp_level: u32,
        auth_info: *mut c_void,
        capabilities: u32,
    ) -> i32;
}

#[link(name = "oleaut32")]
extern "system" {
    fn SysAllocStringLen(source: *const u16, length: u32) -> *mut u16;
    fn SysFreeString(value: *mut u16);
    fn SysStringLen(value: *mut u16) -> u32;
    fn VariantClear(variant: *mut Variant) -> i32;
}

#[cfg(test)]
mod tests {
    use super::decode_gm_output_byte;

    #[test]
    fn decodes_amd_shifted_gm_output_bytes() {
        assert_eq!(decode_gm_output_byte(0x73_00), 0x73);
        assert_eq!(decode_gm_output_byte(0x01_00), 0x01);
        assert_eq!(decode_gm_output_byte(0x04_00), 0x04);
        assert_eq!(decode_gm_output_byte(0x05_00), 0x05);
    }

    #[test]
    fn keeps_legacy_low_byte_gm_outputs() {
        assert_eq!(decode_gm_output_byte(0x00), 0x00);
        assert_eq!(decode_gm_output_byte(0x01), 0x01);
        assert_eq!(decode_gm_output_byte(0x64), 0x64);
    }
}
