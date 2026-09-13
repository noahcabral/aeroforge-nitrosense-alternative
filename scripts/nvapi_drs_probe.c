#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <wchar.h>
#include <wctype.h>

typedef uint32_t NvU32;
typedef uint16_t NvU16;
typedef int32_t NvAPI_Status;

#define NVAPI_OK 0
#define NVAPI_SHORT_STRING_MAX 64
#define NVAPI_UNICODE_STRING_MAX 2048

typedef char NvAPI_ShortString[NVAPI_SHORT_STRING_MAX];
typedef NvU16 NvAPI_UnicodeString[NVAPI_UNICODE_STRING_MAX];

typedef void *(__cdecl *NvAPI_QueryInterface_t)(unsigned int interface_id);
typedef NvAPI_Status(__cdecl *NvAPI_Initialize_t)(void);
typedef NvAPI_Status(__cdecl *NvAPI_Unload_t)(void);
typedef NvAPI_Status(__cdecl *NvAPI_GetErrorMessage_t)(NvAPI_Status status, NvAPI_ShortString message);
typedef NvAPI_Status(__cdecl *NvAPI_DRS_EnumAvailableSettingIds_t)(NvU32 *setting_ids, NvU32 *max_count);
typedef NvAPI_Status(__cdecl *NvAPI_DRS_GetSettingNameFromId_t)(NvU32 setting_id, NvAPI_UnicodeString *setting_name);

static int contains_token(const wchar_t *text, const wchar_t *token) {
    return wcsstr(text, token) != NULL;
}

static int matches_interesting_name(const wchar_t *name) {
    static const wchar_t *tokens[] = {
        L"whisper",
        L"quiet",
        L"boost",
        L"fps",
        L"frame",
        L"battery",
        L"acoustic",
        L"noise",
        L"power",
        L"thermal",
        L"spl",
        L"topps",
        L"rise",
    };
    wchar_t lowered[NVAPI_UNICODE_STRING_MAX] = {0};
    size_t i = 0;

    for (; name[i] != L'\0' && i < NVAPI_UNICODE_STRING_MAX - 1; ++i) {
        lowered[i] = (wchar_t)towlower(name[i]);
    }
    lowered[i] = L'\0';

    for (i = 0; i < sizeof(tokens) / sizeof(tokens[0]); ++i) {
        if (contains_token(lowered, tokens[i])) {
            return 1;
        }
    }

    return 0;
}

static void print_nvapi_error(NvAPI_GetErrorMessage_t get_error_message, const char *context, NvAPI_Status status) {
    NvAPI_ShortString message = {0};
    if (get_error_message != NULL) {
        get_error_message(status, message);
        fprintf(stderr, "%s failed: %s (%d)\n", context, message, status);
    } else {
        fprintf(stderr, "%s failed: %d\n", context, status);
    }
}

int wmain(void) {
    SetDllDirectoryW(
        L"C:\\Windows\\System32\\DriverStore\\FileRepository\\nvaci.inf_amd64_4f09720abbfe8b39\\Display.NvContainer");
    LoadLibraryW(L"NvMessageBus.dll");

    HMODULE nvapi = LoadLibraryW(L"C:\\Windows\\System32\\nvapi64.dll");
    if (nvapi == NULL) {
        fwprintf(stderr, L"failed to load nvapi64.dll: %lu\n", GetLastError());
        return 1;
    }

    NvAPI_QueryInterface_t query_interface =
        (NvAPI_QueryInterface_t)GetProcAddress(nvapi, "nvapi_QueryInterface");
    if (query_interface == NULL) {
        fprintf(stderr, "failed to resolve nvapi_QueryInterface\n");
        return 1;
    }

    NvAPI_Initialize_t nvapi_initialize = (NvAPI_Initialize_t)query_interface(0x0150e828);
    NvAPI_Unload_t nvapi_unload = (NvAPI_Unload_t)query_interface(0xd22bdd7e);
    NvAPI_GetErrorMessage_t nvapi_get_error_message =
        (NvAPI_GetErrorMessage_t)query_interface(0x6c2d048c);
    NvAPI_DRS_EnumAvailableSettingIds_t enum_setting_ids =
        (NvAPI_DRS_EnumAvailableSettingIds_t)query_interface(0xf020614a);
    NvAPI_DRS_GetSettingNameFromId_t get_setting_name =
        (NvAPI_DRS_GetSettingNameFromId_t)query_interface(0xd61cbe6e);

    if (!nvapi_initialize || !nvapi_unload || !enum_setting_ids || !get_setting_name) {
        fprintf(stderr, "failed to resolve one or more NVAPI entrypoints\n");
        return 1;
    }

    NvAPI_Status status = nvapi_initialize();
    if (status != NVAPI_OK) {
        print_nvapi_error(nvapi_get_error_message, "NvAPI_Initialize", status);
        return 1;
    }

    NvU32 count = 16384;
    NvU32 *setting_ids = (NvU32 *)calloc(count, sizeof(NvU32));
    if (setting_ids == NULL) {
        fprintf(stderr, "failed to allocate settings buffer\n");
        nvapi_unload();
        return 1;
    }

    status = enum_setting_ids(setting_ids, &count);
    if (status != NVAPI_OK) {
        print_nvapi_error(nvapi_get_error_message, "NvAPI_DRS_EnumAvailableSettingIds", status);
        free(setting_ids);
        nvapi_unload();
        return 1;
    }

    printf("available_setting_count=%u\n", count);
    for (NvU32 i = 0; i < count; ++i) {
        NvAPI_UnicodeString setting_name = {0};
        status = get_setting_name(setting_ids[i], &setting_name);
        if (status != NVAPI_OK) {
            continue;
        }

        if (matches_interesting_name((const wchar_t *)setting_name)) {
            wprintf(L"0x%08X %ls\n", setting_ids[i], (const wchar_t *)setting_name);
        }
    }

    free(setting_ids);
    nvapi_unload();
    return 0;
}
