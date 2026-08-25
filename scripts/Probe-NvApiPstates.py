from __future__ import annotations

import argparse
import ctypes
import json
from pathlib import Path


NVAPI_MAX_PHYSICAL_GPUS = 64
NVAPI_SHORT_STRING_MAX = 64
NVAPI_MAX_GPU_PSTATE20_PSTATES = 16
NVAPI_MAX_GPU_PSTATE20_CLOCKS = 8
NVAPI_MAX_GPU_PSTATE20_BASE_VOLTAGES = 4

NVAPI_OK = 0
NVAPI_GPU_PUBLIC_CLOCK_GRAPHICS = 0
NVAPI_GPU_PUBLIC_CLOCK_MEMORY = 4
NVAPI_GPU_PUBLIC_CLOCK_PROCESSOR = 7
NVAPI_GPU_PUBLIC_CLOCK_VIDEO = 8
NVAPI_GPU_PERF_VOLTAGE_INFO_DOMAIN_CORE = 0


def make_nvapi_version(struct_type: type[ctypes.Structure], version: int) -> int:
    return ctypes.sizeof(struct_type) | (version << 16)


class NvGpuPerfPstates20ParamDelta(ctypes.Structure):
    _fields_ = [
        ("value", ctypes.c_int32),
        ("min", ctypes.c_int32),
        ("max", ctypes.c_int32),
    ]


class NvGpuPstate20ClockEntrySingle(ctypes.Structure):
    _fields_ = [("freq_kHz", ctypes.c_uint32)]


class NvGpuPstate20ClockEntryRange(ctypes.Structure):
    _fields_ = [
        ("minFreq_kHz", ctypes.c_uint32),
        ("maxFreq_kHz", ctypes.c_uint32),
        ("domainId", ctypes.c_uint32),
        ("minVoltage_uV", ctypes.c_uint32),
        ("maxVoltage_uV", ctypes.c_uint32),
    ]


class NvGpuPstate20ClockEntryData(ctypes.Union):
    _fields_ = [
        ("single", NvGpuPstate20ClockEntrySingle),
        ("range", NvGpuPstate20ClockEntryRange),
    ]


class NvGpuPstate20ClockEntryV1(ctypes.Structure):
    _fields_ = [
        ("domainId", ctypes.c_uint32),
        ("typeId", ctypes.c_uint32),
        ("bIsEditable", ctypes.c_uint32, 1),
        ("reserved", ctypes.c_uint32, 31),
        ("freqDelta_kHz", NvGpuPerfPstates20ParamDelta),
        ("data", NvGpuPstate20ClockEntryData),
    ]


class NvGpuPstate20BaseVoltageEntryV1(ctypes.Structure):
    _fields_ = [
        ("domainId", ctypes.c_uint32),
        ("bIsEditable", ctypes.c_uint32, 1),
        ("reserved", ctypes.c_uint32, 31),
        ("volt_uV", ctypes.c_uint32),
        ("voltDelta_uV", NvGpuPerfPstates20ParamDelta),
    ]


class NvGpuPerfPstate20InfoPstate(ctypes.Structure):
    _fields_ = [
        ("pstateId", ctypes.c_uint32),
        ("bIsEditable", ctypes.c_uint32, 1),
        ("reserved", ctypes.c_uint32, 31),
        ("clocks", NvGpuPstate20ClockEntryV1 * NVAPI_MAX_GPU_PSTATE20_CLOCKS),
        ("baseVoltages", NvGpuPstate20BaseVoltageEntryV1 * NVAPI_MAX_GPU_PSTATE20_BASE_VOLTAGES),
    ]


class NvGpuPerfPstate20InfoOv(ctypes.Structure):
    _fields_ = [
        ("numVoltages", ctypes.c_uint32),
        ("voltages", NvGpuPstate20BaseVoltageEntryV1 * NVAPI_MAX_GPU_PSTATE20_BASE_VOLTAGES),
    ]


class NvGpuPerfPstates20InfoV2(ctypes.Structure):
    _fields_ = [
        ("version", ctypes.c_uint32),
        ("bIsEditable", ctypes.c_uint32, 1),
        ("reserved", ctypes.c_uint32, 31),
        ("numPstates", ctypes.c_uint32),
        ("numClocks", ctypes.c_uint32),
        ("numBaseVoltages", ctypes.c_uint32),
        ("pstates", NvGpuPerfPstate20InfoPstate * NVAPI_MAX_GPU_PSTATE20_PSTATES),
        ("ov", NvGpuPerfPstate20InfoOv),
    ]


def clock_domain_name(domain_id: int) -> str:
    return {
        NVAPI_GPU_PUBLIC_CLOCK_GRAPHICS: "graphics",
        NVAPI_GPU_PUBLIC_CLOCK_MEMORY: "memory",
        NVAPI_GPU_PUBLIC_CLOCK_PROCESSOR: "processor",
        NVAPI_GPU_PUBLIC_CLOCK_VIDEO: "video",
    }.get(domain_id, f"domain_{domain_id}")


def voltage_domain_name(domain_id: int) -> str:
    return {
        NVAPI_GPU_PERF_VOLTAGE_INFO_DOMAIN_CORE: "core",
    }.get(domain_id, f"domain_{domain_id}")


def pstate_name(pstate_id: int) -> str:
    if 0 <= pstate_id <= 15:
        return f"P{pstate_id}"
    return f"unknown_{pstate_id}"


def decode_clock_entry(entry: NvGpuPstate20ClockEntryV1) -> dict[str, object]:
    result: dict[str, object] = {
        "domainId": int(entry.domainId),
        "domainName": clock_domain_name(int(entry.domainId)),
        "typeId": int(entry.typeId),
        "isEditable": bool(entry.bIsEditable),
        "freqDelta_kHz": {
            "value": int(entry.freqDelta_kHz.value),
            "min": int(entry.freqDelta_kHz.min),
            "max": int(entry.freqDelta_kHz.max),
        },
    }
    if int(entry.typeId) == 0:
        result["single"] = {"freq_kHz": int(entry.data.single.freq_kHz)}
    elif int(entry.typeId) == 1:
        result["range"] = {
            "minFreq_kHz": int(entry.data.range.minFreq_kHz),
            "maxFreq_kHz": int(entry.data.range.maxFreq_kHz),
            "voltageDomainId": int(entry.data.range.domainId),
            "voltageDomainName": voltage_domain_name(int(entry.data.range.domainId)),
            "minVoltage_uV": int(entry.data.range.minVoltage_uV),
            "maxVoltage_uV": int(entry.data.range.maxVoltage_uV),
        }
    return result


def decode_voltage_entry(entry: NvGpuPstate20BaseVoltageEntryV1) -> dict[str, object]:
    return {
        "domainId": int(entry.domainId),
        "domainName": voltage_domain_name(int(entry.domainId)),
        "isEditable": bool(entry.bIsEditable),
        "volt_uV": int(entry.volt_uV),
        "voltDelta_uV": {
            "value": int(entry.voltDelta_uV.value),
            "min": int(entry.voltDelta_uV.min),
            "max": int(entry.voltDelta_uV.max),
        },
    }


def decode_pstates(info: NvGpuPerfPstates20InfoV2) -> dict[str, object]:
    decoded_pstates: list[dict[str, object]] = []
    for pstate_index in range(min(int(info.numPstates), NVAPI_MAX_GPU_PSTATE20_PSTATES)):
        pstate = info.pstates[pstate_index]
        decoded_pstates.append(
            {
                "index": pstate_index,
                "pstateId": int(pstate.pstateId),
                "pstateName": pstate_name(int(pstate.pstateId)),
                "isEditable": bool(pstate.bIsEditable),
                "clocks": [
                    decode_clock_entry(pstate.clocks[clock_index])
                    for clock_index in range(min(int(info.numClocks), NVAPI_MAX_GPU_PSTATE20_CLOCKS))
                ],
                "baseVoltages": [
                    decode_voltage_entry(pstate.baseVoltages[voltage_index])
                    for voltage_index in range(min(int(info.numBaseVoltages), NVAPI_MAX_GPU_PSTATE20_BASE_VOLTAGES))
                ],
            }
        )

    ov_voltages = [
        decode_voltage_entry(info.ov.voltages[index])
        for index in range(min(int(info.ov.numVoltages), NVAPI_MAX_GPU_PSTATE20_BASE_VOLTAGES))
    ]

    return {
        "isEditable": bool(info.bIsEditable),
        "numPstates": int(info.numPstates),
        "numClocks": int(info.numClocks),
        "numBaseVoltages": int(info.numBaseVoltages),
        "ov": {
            "numVoltages": int(info.ov.numVoltages),
            "voltages": ov_voltages,
        },
        "pstates": decoded_pstates,
    }


def status_name(code: int) -> str:
    names = {
        0: "NVAPI_OK",
        -1: "NVAPI_ERROR",
        -2: "NVAPI_LIBRARY_NOT_FOUND",
        -3: "NVAPI_NO_IMPLEMENTATION",
        -4: "NVAPI_API_NOT_INITIALIZED",
        -5: "NVAPI_INVALID_ARGUMENT",
        -6: "NVAPI_NVIDIA_DEVICE_NOT_FOUND",
        -8: "NVAPI_INVALID_HANDLE",
        -9: "NVAPI_INCOMPATIBLE_STRUCT_VERSION",
        -101: "NVAPI_EXPECTED_PHYSICAL_GPU_HANDLE",
        -104: "NVAPI_NOT_SUPPORTED",
    }
    return names.get(code, f"NVAPI_STATUS_{code}")


def main() -> int:
    parser = argparse.ArgumentParser(description="Read NVAPI Pstates20 information for local GPU correlation.")
    parser.add_argument(
        "--out",
        default=r"C:\Users\noah\Desktop\nvapi-pstates-probe.json",
        help="Path to write probe JSON",
    )
    args = parser.parse_args()

    nvapi = ctypes.WinDLL("nvapi64.dll")
    query_interface = nvapi.nvapi_QueryInterface
    query_interface.argtypes = [ctypes.c_uint32]
    query_interface.restype = ctypes.c_void_p

    def load_interface(interface_id: int, restype, argtypes):
        proc = query_interface(interface_id)
        if not proc:
            raise SystemExit(f"nvapi_QueryInterface failed for 0x{interface_id:08x}")
        func_type = ctypes.CFUNCTYPE(restype, *argtypes)
        return func_type(proc)

    NvAPI_Initialize = load_interface(0x0150E828, ctypes.c_int32, [])
    NvAPI_Unload = load_interface(0xD22BDD7E, ctypes.c_int32, [])
    NvAPI_GetErrorMessage = load_interface(
        0x6C2D048C, ctypes.c_int32, [ctypes.c_int32, ctypes.c_char * NVAPI_SHORT_STRING_MAX]
    )
    NvAPI_EnumPhysicalGPUs = load_interface(
        0xE5AC921F, ctypes.c_int32, [ctypes.c_void_p * NVAPI_MAX_PHYSICAL_GPUS, ctypes.POINTER(ctypes.c_uint32)]
    )
    NvAPI_GPU_GetFullName = load_interface(
        0xCEEE8E9F, ctypes.c_int32, [ctypes.c_void_p, ctypes.c_char * NVAPI_SHORT_STRING_MAX]
    )
    NvAPI_GPU_GetCurrentPstate = load_interface(
        0x927DA4F6, ctypes.c_int32, [ctypes.c_void_p, ctypes.POINTER(ctypes.c_uint32)]
    )
    NvAPI_GPU_GetPstates20 = load_interface(
        0x6FF81213, ctypes.c_int32, [ctypes.c_void_p, ctypes.POINTER(NvGpuPerfPstates20InfoV2)]
    )

    def error_message(code: int) -> str:
        buffer = (ctypes.c_char * NVAPI_SHORT_STRING_MAX)()
        message_status = int(NvAPI_GetErrorMessage(code, buffer))
        if message_status == NVAPI_OK:
            return bytes(buffer).split(b"\x00", 1)[0].decode("ascii", "ignore")
        return status_name(code)

    init_status = int(NvAPI_Initialize())
    if init_status != NVAPI_OK:
        raise SystemExit(f"NVAPI initialize failed: {status_name(init_status)} - {error_message(init_status)}")

    gpu_handles = (ctypes.c_void_p * NVAPI_MAX_PHYSICAL_GPUS)()
    gpu_count = ctypes.c_uint32(0)
    enum_status = int(NvAPI_EnumPhysicalGPUs(gpu_handles, ctypes.byref(gpu_count)))
    if enum_status != NVAPI_OK:
        NvAPI_Unload()
        raise SystemExit(f"GPU enumeration failed: {status_name(enum_status)} - {error_message(enum_status)}")

    gpus: list[dict[str, object]] = []
    for index in range(int(gpu_count.value)):
        handle = gpu_handles[index]
        name_buffer = (ctypes.c_char * NVAPI_SHORT_STRING_MAX)()
        name_status = int(NvAPI_GPU_GetFullName(handle, name_buffer))
        gpu_name = bytes(name_buffer).split(b"\x00", 1)[0].decode("ascii", "ignore")

        current_pstate = ctypes.c_uint32(0)
        current_pstate_status = int(NvAPI_GPU_GetCurrentPstate(handle, ctypes.byref(current_pstate)))

        pstates_info = NvGpuPerfPstates20InfoV2()
        pstates_attempts: list[dict[str, object]] = []
        pstates_result: dict[str, object] | None = None
        for version in (3, 2, 1):
            pstates_info = NvGpuPerfPstates20InfoV2()
            pstates_info.version = make_nvapi_version(NvGpuPerfPstates20InfoV2, version)
            pstates_status = int(NvAPI_GPU_GetPstates20(handle, ctypes.byref(pstates_info)))
            attempt = {
                "version": version,
                "status": pstates_status,
                "statusName": status_name(pstates_status),
                "statusMessage": error_message(pstates_status),
            }
            pstates_attempts.append(attempt)
            if pstates_status == NVAPI_OK:
                pstates_result = decode_pstates(pstates_info)
                break

        gpus.append(
            {
                "index": index,
                "handle": hex(int(ctypes.cast(handle, ctypes.c_void_p).value or 0)),
                "nameStatus": {
                    "code": name_status,
                    "name": status_name(name_status),
                    "message": error_message(name_status),
                },
                "name": gpu_name,
                "currentPstate": {
                    "status": current_pstate_status,
                    "statusName": status_name(current_pstate_status),
                    "statusMessage": error_message(current_pstate_status),
                    "id": int(current_pstate.value),
                    "name": pstate_name(int(current_pstate.value)),
                },
                "pstates20Attempts": pstates_attempts,
                "pstates20": pstates_result,
            }
        )

    unload_status = int(NvAPI_Unload())

    result = {
        "initStatus": {
            "code": init_status,
            "name": status_name(init_status),
            "message": error_message(init_status),
        },
        "enumStatus": {
            "code": enum_status,
            "name": status_name(enum_status),
            "message": error_message(enum_status),
        },
        "gpuCount": int(gpu_count.value),
        "structureSizes": {
            "NvGpuPerfPstates20InfoV2": ctypes.sizeof(NvGpuPerfPstates20InfoV2),
            "NvGpuPerfPstate20InfoPstate": ctypes.sizeof(NvGpuPerfPstate20InfoPstate),
            "NvGpuPstate20ClockEntryV1": ctypes.sizeof(NvGpuPstate20ClockEntryV1),
            "NvGpuPstate20BaseVoltageEntryV1": ctypes.sizeof(NvGpuPstate20BaseVoltageEntryV1),
        },
        "gpus": gpus,
        "unloadStatus": {
            "code": unload_status,
            "name": status_name(unload_status),
            "message": error_message(unload_status) if unload_status != NVAPI_OK else status_name(unload_status),
        },
        "notes": [
            "This probe is read-only and uses public exported NVAPI entry points.",
            "If Pstates20 succeeds, the result should expose the editable clock and voltage domains the driver reports.",
            "The next clean-room step is to compare these live domains against the Acer HID OC profile rows.",
        ],
    }

    Path(args.out).write_text(json.dumps(result, indent=2), encoding="utf-8")
    print(f"Wrote {args.out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
