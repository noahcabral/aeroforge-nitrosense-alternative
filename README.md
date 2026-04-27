# AeroForge Control

Premium-feeling frontend prototype for a laptop fan control and battery or power management application. The project now includes a Tauri desktop shell around the React UI and a Windows service for hardware-facing control paths.

## Included UI flows

- Custom fan curves with draggable CPU and GPU thermal nodes
- Power profile switching
- Fan profile switching
- Smart charging toggle with an 80% charge cap preview
- Boot splash image swapping with preset art and custom upload preview

## Tech stack

- React 19
- TypeScript
- Vite
- Tauri 2

## Run locally

```powershell
npm.cmd install
npm.cmd run dev
```

## Production build

```powershell
npm.cmd run build
```

## Run as a desktop app

```powershell
npm.cmd run tauri:dev
```

## Windows prerequisites for Tauri

- Rust toolchain via `rustup`
- Visual Studio Build Tools with MSVC and Windows SDK components
- WebView2 runtime

## Package the desktop app

```powershell
npm.cmd run tauri:build
```

## Create a portable folder

```powershell
npm.cmd run portable:build
```

This creates:

- `portable\AeroForge Control Portable\`
- `portable\AeroForge-Control-Portable-0.12.0.zip`

## Install the Nitro key helper

```powershell
npm.cmd run startup:install
```

This registers `aeroforge-hotkey-helper.exe --daemon` in the logged-in user session so the physical Nitro key can open or focus AeroForge without keeping the WebView UI resident. To remove it:

```powershell
npm.cmd run startup:uninstall
```

## Notes for backend wiring later

- `src/App.tsx` centralizes the mock state for all primary controls.
- Fan curves are represented as temperature and speed points for both CPU and GPU zones.
- Boot image upload uses `URL.createObjectURL()` for local preview only.
- Charge-limit controls are safe UI state changes only.
- `src-tauri/src/backend/` now contains the typed backend contract, capability snapshot, control snapshot, telemetry snapshot, and persistence-backed desktop state models exposed through Tauri commands.
- The backend now persists AeroForge-owned control state to disk and routes power, GPU tuning, and fan writes through the AeroForge service.

## AeroForge Windows service

The repo now includes a separate barebones Windows service host under `aeroforge-service/`.

Current shape:

- one AeroForge-owned Windows service process
- a thin supervisor that owns lifecycle and worker health only
- parallel worker threads for capability, persistence, telemetry, and named-pipe IPC
- worker snapshot files under `ProgramData\\AeroForge\\Service\\state`
- a supervisor snapshot at `ProgramData\\AeroForge\\Service\\state\\supervisor.json`
- local IPC over `\\.\pipe\AeroForgeService` instead of localhost ports
- no dependency on Acer localhost services or other vendor IPC

Current fan control path:

- fan profile and custom-curve apply requests flow through the AeroForge service
- the service calls `ROOT\\WMI` `AcerGamingFunction` directly on supported Acer Nitro hardware
- `SetGamingFanBehavior` receives Acer behavior inputs for auto, custom, and max fan profiles
- `SetGamingFanSpeed` receives per-fan target inputs for CPU and GPU fan percentages
- when Custom is active, the service re-reads telemetry and reapplies the curve-derived CPU/GPU speed targets every 5 seconds
- RPM movement is verified separately through telemetry rather than trusting the write call alone
- no NitroSense websocket, AcerAgentService PSSDK socket, or PredatorSense pipe dependency is used for AeroForge fan writes

Current power control path:

- Balanced and Turbo use direct `ROOT\\WMI` `AcerGamingFunction` profile writes
- Custom uses the direct balanced firmware base and then layers the requested Windows processor-state policy
- Quiet uses a direct NVIDIA NVAPI Whisper path with the verified init-plus-setter sequence instead of NitroSense runtime dependencies
- Windows processor-state policy is still applied through `powercfg` and read back afterward for AC/DC verification

Current read-only telemetry coverage:

- Windows power status for battery percentage and AC state
- Windows system CPU time sampling for CPU usage
- standard Windows processor queries for current CPU clock
- NVIDIA NVML for GPU utilization, temperature, and graphics clock when available
- direct HID status reads for CPU and GPU fan speed on supported Nitro hardware
- ACPI thermal-zone data for platform thermals on supported systems
- independent CPU-package and system-board thermal separation is still incomplete until AeroForge adds deeper EC or ACPI decoding

Clean-room boundary:

- AeroForge source does not include Acer source code, decompiled Acer code, or Acer binary string-analysis artifacts
- Vendor names, WMI class names, method names, and numeric inputs are treated as runtime-observed interface facts

Useful commands:

```powershell
npm.cmd run service:build
npm.cmd run service:console
```

Install and uninstall scripts:

```powershell
powershell -ExecutionPolicy Bypass -File scripts/Install-AeroForgeService.ps1
powershell -ExecutionPolicy Bypass -File scripts/Uninstall-AeroForgeService.ps1
```

The install script registers `AeroForgeService` with delayed automatic startup so the service does not race early-boot NVIDIA initialization.
