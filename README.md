# AeroForge Control

## Bug Reports And Feedback

If AeroForge breaks, behaves oddly, or you want to report a bug or leave feedback, go here first:

[https://discord.gg/EuWpmkjQY8](https://discord.gg/EuWpmkjQY8)

This GitHub repository is not monitored for support, bug reports, or pull requests. Please do not open pull requests; use the Discord above for feedback and issue reports.

Laptop fan control and battery / power management for Acer Nitro hardware. Three tiers: a React UI, a Tauri desktop shell (Rust) that owns persistence and the tray, and a Windows service (Rust) that performs the hardware-facing WMI, HID, NVML and power writes.

## Included UI flows

- Custom fan curves with draggable CPU and GPU thermal nodes
- Power profile switching
- Fan profile switching
- Smart charging toggle with an 80% charge cap preview
- Boot splash image swapping with preset art and custom upload preview

## Tech stack

- React 19
- TypeScript (strict)
- Vite + Vitest
- Tauri 2
- Rust (Tauri shell and Windows service)

## Repository layout

- `src/` React UI. `src/lib/` holds pure, tested modules (curve math, preset and settings import/export, formatting, keyboard shortcuts); `src/components/` holds presentational components.
- `src-tauri/` Tauri shell: Tauri commands, control-state persistence, tray, updater, Nitro key helper.
- `aeroforge-service/` Windows service: supervisor plus capability, persistence, telemetry and named-pipe IPC workers.
- `scripts/` install, packaging, release and support-bundle scripts.
- `third_party/` NVAPI headers and the bundled PawnIO runtime.

## Run locally

```powershell
npm.cmd install
npm.cmd run dev
```

## Production build

```powershell
npm.cmd run build
```

## Tests and lint

```powershell
npm.cmd test
npm.cmd run lint
cargo test --manifest-path aeroforge-service/Cargo.toml
cargo test --lib --manifest-path src-tauri/Cargo.toml
```

Frontend tests live beside the module they cover (`src/lib/*.test.ts`) and run in plain Node; `npm.cmd run test:watch` keeps them running while you edit.

## Run as a desktop app

```powershell
npm.cmd run tauri:dev
```

## Windows prerequisites for Tauri

- Node.js 22.12 or newer
- Rust toolchain via `rustup`
- Visual Studio Build Tools with MSVC and Windows SDK components
- WebView2 runtime

The build hooks find `node.exe` and `cargo.exe` on `PATH` first, then in the standard installer locations. If either lives somewhere unusual, set `AEROFORGE_NODE` or `AEROFORGE_CARGO` to the full executable path.

## Working from a cloud-synced folder

If the checkout lives on Google Drive, OneDrive or a similar virtual filesystem:

- Vite keeps its dependency cache in the OS temp directory (`AEROFORGE_VITE_CACHE_DIR` overrides it) and the dev server skips the Rust `target/` trees when watching, because walking tens of thousands of build artifacts through a cloud filesystem stalls the server.
- `npm install` can fail with `EBADF` or `UNKNOWN: write` because cloud filesystems reject the way npm's tar extractor writes files. Run the install in a local folder with a copy of `package.json` and `package-lock.json`, then copy the new packages plus `node_modules/.package-lock.json` back.
- Cargo prints "hard linking files in the incremental compilation cache failed" on cloud mounts and falls back to copying. It is harmless but slows rebuilds. The in-tree `target/` folders are still used because the packaging scripts and `tauri.conf.json` reference them by path.
- Cloud mounts do not support directory junctions or symlinks, so `node_modules` cannot be redirected to local disk. For day-to-day development a local clone is faster; use the cloud folder or GitHub as the backup.

## Package the desktop app

```powershell
npm.cmd run tauri:build
```

This builds the frontend, the Windows service and the hotkey helper (`scripts/Run-BeforeBuild.ps1`) before compiling the shell, because Tauri validates the bundled resource executables at compile time. The installer lands in `src-tauri\target\release\bundle\nsis\`.

## Create a portable folder

```powershell
npm.cmd run portable:build
```

This creates:

- `portable\AeroForge Control Portable\`
- `portable\AeroForge-Control-Portable-<version>.zip` (version taken from `package.json`)

## Install the Nitro key helper

```powershell
npm.cmd run startup:install
```

This registers `aeroforge-hotkey-helper.exe --daemon` in the logged-in user session so the physical Nitro key can open or focus AeroForge without keeping the WebView UI resident. To remove it:

```powershell
npm.cmd run startup:uninstall
```

## Support debug bundle

For users who cannot get AeroForge working, send them:

```text
scripts\AeroForge-Debug-Collector.cmd
```

The portable ZIP also includes the same `AeroForge-Debug-Collector.cmd` beside
`aeroforge-control.exe`. They can double-click it or run it from Command Prompt. It creates an
`AeroForge-Debug-YYYYMMDD-HHMMSS.zip` on their Desktop with service status,
named-pipe read-only probes, AeroForge logs and state snapshots, filtered Acer /
Nitro / NVIDIA / WebView diagnostics, Windows event logs, power/display state,
driver inventory, Defender status, startup entries, shortcut targets, update
reachability, NVIDIA power-limit readback, AMD/CPU power diagnostics, read-only
Acer WMI probes, app/service version facts, installer logs, UI performance logs,
dxdiag output, filtered Windows Error Reporting crash inventories, and a root
`summary.json`.

The collector is read-only. It skips binaries, images, staged update payloads,
WebView cache/storage folders, and token-like filenames, then redacts common
token/password/secret strings in copied text files. Launch it with
`-Quick -PollSeconds 60` for a low-noise service/CPU/power/fan timeline, or
`-Deep -PollSeconds 120 -PollIntervalMs 1000` for the full support timeline.
Focused polling is also available, for example
`-Poll fans pipe performance -PollSeconds 45 -PollIntervalMs 500` for custom fan
or mode-switch lag, and `-Poll nvidia gpu-counters processes -PollSeconds 90`
for dGPU idle or power-limit reports. Add `-NoNvidiaSmi` when testing whether
NVIDIA command-line polling itself is waking the dGPU. Maintainers can add
`-OutputRoot "C:\Temp\AeroForgeDebug"` to place the bundle somewhere other than
the Desktop, or run `-ListOptions` to print every supported switch.

## Frontend architecture notes

- `src/App.tsx` is still the single owner of UI state for every tab; it talks to the Tauri shell through `src/lib/backend.ts`, which is the typed contract mirrored by `src-tauri/src/backend/models.rs`.
- Fan curves are temperature/speed points for the CPU and GPU zones. `src/lib/curves.ts` owns the invariants (sorted, at least 2C apart, non-decreasing speed) and every curve that reaches the service passes through `normalizeCurvePoints`.
- Boot image upload uses `URL.createObjectURL()` for the preview; `src/lib/bootLogo.ts` renders the firmware payload.
- `src-tauri/src/backend/` holds the typed backend contract, capability snapshot, control snapshot, telemetry snapshot, and persistence-backed desktop state exposed through Tauri commands. Power, GPU tuning, and fan writes are routed through the AeroForge service.
- Hooks under `src/hooks/` own self-contained state: `useToasts`, `usePerformanceLog`, `useFrameStats` (rAF driver over the tested `lib/frameSampler`), `useDebugLog`. Remaining split, in order: the backend polling/bootstrap loop into a hook; each tab's JSX into a component under `src/components/`; the fan-curve editor owning drag state so a drag only re-renders the chart.
- Backend responses are applied to UI state only for the fields that command is authoritative for (`applyAuthoritativeSnapshot`); bootstrap, tray events and settings import apply the whole snapshot. Persistence is gated until the bootstrap snapshot has loaded so an early click cannot write defaults over saved presets.
- Stage auto-scaling (`measureStageFit`) currently runs only in dev builds; the gate looks like a debugging leftover and is left as-is pending a decision.

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
- `SetGamingFanBehavior` receives Acer behavior inputs for auto, max, and mixed custom fan profiles
- `SetGamingFanSpeed` receives per-fan target inputs for CPU and GPU fan percentages
- when Custom is active, the service re-reads telemetry and reapplies the curve-derived CPU/GPU speed targets every 5 seconds
- RPM movement is verified separately through direct Acer sensor telemetry rather than trusting the write call alone
- no NitroSense websocket, AcerAgentService PSSDK socket, or PredatorSense pipe dependency is used for AeroForge fan writes

Current power control path:

- Balanced, Performance, Turbo, and Custom base profiles prefer `AcerGamingFunction.SetGamingMiscSetting(0x0B, profile)` platform-profile writes and fall back to legacy `SetGamingProfile` only if the misc-setting path is unavailable
- Custom layers the requested Windows processor-state policy over the selected firmware base
- Quiet uses Acer platform profile value `0x00` plus the direct NVIDIA NVAPI Whisper path when available
- Windows processor-state policy is still applied through `powercfg` and read back afterward for AC/DC verification
- CPU package power and PL1/PL2 readback use the PawnIO MSR/RAPL path when PawnIO is installed. The AeroForge installer can install the bundled open-source PawnIO setup on request, stages the `IntelMSR.bin` module, detects `PawnIOLib.dll`, and configures the service-level `AEROFORGE_PAWNIO_DLL`, `AEROFORGE_PAWNIO_MODULE`, and `AEROFORGE_ENABLE_PAWNIO` environment variables automatically. If PawnIO is not installed, the CPU watt/PL panel intentionally reports that the CPU power driver is missing.
- The legacy WinRing0 CPU MSR/RAPL path is not bundled or enabled in release builds because Windows Defender commonly flags that driver. For diagnostics only, maintainers can set `AEROFORGE_ENABLE_WINRING0=1` and provide a trusted external driver through `AEROFORGE_WINRING0_DRIVER`.

Bundled PawnIO runtime:

- `third_party/pawnio/PawnIO_setup.exe` is the open-source PawnIO setup from `namazso/PawnIO.Setup` release `2.2.0`.
- SHA256: `1F519A22E47187F70A1379A48CA604981C4FCF694F4E65B734AAA74A9FBA3032`.
- Setup is invoked only when the user opts into CPU wattage and PL1/PL2 support during AeroForge setup; silent installs configure an existing PawnIO install but do not force-install PawnIO.

Current read-only telemetry coverage:

- Windows power status for battery percentage and AC state
- Windows system CPU time sampling for CPU usage
- standard Windows processor queries for current CPU clock
- NVIDIA GPU temperature, utilization, clocks, and VRAM are read through NVML only while Windows reports active dedicated-GPU memory use, plus a short cooldown window. Windows counters are used only as the wake gate, not as displayed utilization. NVIDIA power draw and power-limit readback remain disabled by default to avoid the more aggressive polling path; maintainers can opt into that diagnostic power telemetry with `AEROFORGE_ENABLE_NVIDIA_TELEMETRY=1`.
- direct `AcerGamingFunction.GetGamingSysInfo` reads for CPU/GPU/system temperatures and CPU/GPU fan RPMs when available
- direct HID status reads for CPU and GPU fan speed on supported Nitro hardware
- ACPI thermal-zone data as fallback platform thermals on supported systems
- independent CPU-package and system-board thermal separation is still incomplete until AeroForge adds deeper EC or ACPI decoding

Trust model:

- The service runs as SYSTEM. `C:\ProgramData\AeroForge\Service` is restricted to SYSTEM and Administrators (full) and Users (read/execute); both installers set this and the service re-applies it at startup. Child tools (`powercfg`, `powershell`, `reg`, `nvidia-smi`) and `nvapi64.dll` are launched or loaded by absolute System32 path so nothing dropped next to the service binary can shadow them.
- The named pipe grants Authenticated Users read/write, so on a shared machine any signed-in local user can change fan and power state. The write token in `state\ipc-write-token.txt` is world-readable and does not change that; it exists to keep the desktop app and service in sync, not as an access control. Restricting control to one account would need the token created service-side with a per-user ACL, or the pipe SDDL narrowed.
- Requests are capped at 32 MB, concurrent clients at 16, fan curves at 32 points; the pipe name is never shared with another process.
- Updates are verified against the release's SHA256 sidecar (integrity in transit, not authenticity). Signing releases (minisign/Authenticode with a pinned key) is the missing piece for authenticity. The updater currently follows `noahcabral/aeroforge-nitrosense-alternative`; `scripts/Publish-GitHubRelease.ps1` publishes to this checkout's `origin`.

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
