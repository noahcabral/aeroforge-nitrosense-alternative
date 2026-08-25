# AeroForge IPC Protocol Documentation

**Named Pipe:** `\\.\pipe\AeroForgeService`  
**Protocol:** JSON request/response over Windows Named Pipes  
**Encoding:** UTF-8, newline-delimited (`\n`)

---

## Request Format

All requests follow this structure:

```json
{
  "kind": "<CommandName>",
  "payload": { /* optional command-specific data */ }
}
```

## Response Format

```json
// Success
{
  "kind": "ok",
  "payload": { /* command-specific response data */ }
}

// Error
{
  "kind": "error",
  "message": "Error description string"
}
```

---

## Commands

### 1. GetServiceStatus
**Request:**
```json
{ "kind": "getServiceStatus" }
```

**Response:**
```json
{
  "connected": true,
  "pipeName": "\\\\.\\pipe\\AeroForgeService",
  "serviceName": "AeroForgeService",
  "version": "0.16.3",
  "stateDir": "C:\\ProgramData\\AeroForge\\Service\\state",
  "supervisorFile": "C:\\ProgramData\\AeroForge\\Service\\state\\supervisor.json",
  "workerCount": 6,
  "updatedAtUnix": 1724616654,
  "workers": [
    {
      "name": "control-worker",
      "state": "Running",
      "intervalSeconds": 1,
      "lastUpdateUnix": 1724616654,
      "lastError": null
    }
  ],
  "detail": "Connected to the AeroForge service named-pipe host."
}
```

---

### 2. GetCapabilities
**Request:**
```json
{ "kind": "getCapabilities" }
```

**Response:**
```json
{
  "powerProfiles": { "available": true, "writable": true, "requiresElevation": false },
  "fanProfiles": { "available": true, "writable": true, "requiresElevation": false },
  "fanCurves": { "available": true, "writable": true, "requiresElevation": false },
  "smartCharging": { "available": true, "writable": true, "requiresElevation": false },
  "usbPower": { "available": true, "writable": true, "requiresElevation": false },
  "blueLightFilter": { "available": true, "writable": false, "requiresElevation": false },
  "gpuTuning": { "available": true, "writable": true, "requiresElevation": false },
  "bootLogo": { "available": true, "writable": true, "requiresElevation": true },
  "notes": []
}
```

---

### 3. GetTelemetrySnapshot
**Request:**
```json
{ "kind": "getTelemetrySnapshot" }
```

**Response:**
```json
{
  "cpuTempC": 62,
  "cpuTempAverageC": 60,
  "cpuTempLowestCoreC": 58,
  "cpuTempHighestCoreC": 65,
  "gpuTempC": 54,
  "systemTempC": 48,
  "cpuUsagePercent": 35,
  "gpuUsagePercent": 12,
  "gpuMemoryUsagePercent": 8,
  "gpuPowerDrawW": 25.5,
  "gpuPowerLimitW": 140.0,
  "gpuPowerDefaultLimitW": 115.0,
  "gpuPowerMinLimitW": 80.0,
  "gpuPowerMaxLimitW": 140.0,
  "cpuPackagePowerW": 45.2,
  "cpuPl1W": 45.0,
  "cpuPl1Enabled": true,
  "cpuPl2W": 107.0,
  "cpuPl2Enabled": true,
  "cpuPowerLimitLocked": false,
  "cpuName": "13th Gen Intel(R) Core(TM) i7-13700HX",
  "cpuBrand": "Intel",
  "gpuName": "NVIDIA GeForce RTX 4060 Laptop GPU",
  "gpuBrand": "NVIDIA",
  "systemVendor": "Acer",
  "systemModel": "Nitro AN515-58",
  "cpuClockMhz": 3400,
  "gpuClockMhz": 1800,
  "cpuFanRpm": 3200,
  "gpuFanRpm": 2950,
  "batteryPercent": 87,
  "batteryLifeRemainingSec": 14400,
  "acPluggedIn": true,
  "heartbeat": 1724616654
}
```

---

### 4. GetControlSnapshot
**Request:**
```json
{ "kind": "getControlSnapshot" }
```

**Response:**
```json
{
  "service": "control-worker",
  "powerApplySupported": true,
  "gpuTuningApplySupported": true,
  "fanApplySupported": true,
  "fanCurveApplySupported": true,
  "activePowerProfile": "turbo",
  "processorState": { "minPercent": 100, "maxPercent": 100 },
  "customBaseProfile": null,
  "processorStateControlEnabled": true,
  "nvidiaTelemetryEnabled": true,
  "processorStateReadback": {
    "ac": { "minPercent": 100, "maxPercent": 100 },
    "dc": { "minPercent": 5, "maxPercent": 100 }
  },
  "processorStateDriftDetected": false,
  "lastAppliedAtUnix": 1724616600,
  "lastApplyDetail": "Applied turbo power profile.",
  "lastError": null,
  "activeGpuTuning": {
    "coreClockMhz": 165,
    "memoryClockMhz": 420,
    "voltageOffsetMv": -35,
    "powerLimitPercent": 114,
    "tempLimitC": 83
  },
  "lastGpuTuningAppliedAtUnix": 1724616600,
  "lastGpuTuningDetail": "Applied GPU tuning.",
  "lastGpuTuningError": null,
  "activeFanProfile": "auto",
  "activeFanCurves": null,
  "currentCpuFanSpeedPercent": 45,
  "currentGpuFanSpeedPercent": 40,
  "lastFanAppliedAtUnix": 1724616600,
  "lastFanApplyDetail": "Applied auto fan profile.",
  "lastFanError": null,
  "lastFanReadback": null,
  "quietAutoFanCalibration": {
    "cpu": { "lastPercent": 18, "lastRpm": 2100, "idlePercent": 18, "elevatedPercent": 24 },
    "gpu": { "lastPercent": 16, "lastRpm": 1950, "idlePercent": 16, "elevatedPercent": 22 },
    "lastTargetRpm": 2000,
    "updatedAtUnix": 1724616600
  },
  "quietAutoThermalWarning": null,
  "fanSpeedCalibration": {
    "running": false,
    "status": "Fan speed calibration has not been run.",
    "startedAtUnix": null,
    "updatedAtUnix": null,
    "completedAtUnix": null,
    "currentPercent": null,
    "settleSeconds": 20,
    "points": [],
    "lastError": null
  },
  "bootLogoApplySupported": true,
  "lastBootLogoAppliedAtUnix": null,
  "lastBootLogoApplyDetail": "Boot-logo apply is ready...",
  "lastBootLogoError": null,
  "lastBootLogoReadback": null
}
```

---

### 5. RestoreStartupState
**Request:**
```json
{ "kind": "restoreStartupState" }
```

**Response:**
```json
{
  "detail": "Restored saved AeroForge power and fan state on manual app launch."
}
```

---

### 6. ApplyPowerProfile
**Request:**
```json
{
  "kind": "applyPowerProfile",
  "payload": {
    "profileId": "turbo",
    "processorState": { "minPercent": 100, "maxPercent": 100 },
    "customBaseProfile": null,
    "processorStateControlEnabled": true
  }
}
```

**Valid `profileId` values:**
- `"battery-guard"` (Quiet mode)
- `"balanced"`
- `"performance"`
- `"turbo"`
- `"custom"`

**Valid `customBaseProfile` values (for `custom` profile):**
- `"balanced"`
- `"performance"`
- `"turbo"`

**Response:**
```json
{
  "profileId": "turbo",
  "processorState": { "minPercent": 100, "maxPercent": 100 },
  "customBaseProfile": null,
  "processorStateControlEnabled": true,
  "readback": {
    "ac": { "minPercent": 100, "maxPercent": 100 },
    "dc": { "minPercent": 5, "maxPercent": 100 }
  },
  "driftDetected": false,
  "appliedAtUnix": 1724616654,
  "detail": "Applied turbo power profile with 100% min / 100% max processor state on AC."
}
```

---

### 7. ApplyGpuTuning
**Request:**
```json
{
  "kind": "applyGpuTuning",
  "payload": {
    "tuning": {
      "coreClockMhz": 165,
      "memoryClockMhz": 420,
      "voltageOffsetMv": -35,
      "powerLimitPercent": 114,
      "tempLimitC": 83
    }
  }
}
```

**Response:**
```json
{
  "tuning": {
    "coreClockMhz": 165,
    "memoryClockMhz": 420,
    "voltageOffsetMv": -35,
    "powerLimitPercent": 114,
    "tempLimitC": 83
  },
  "appliedAtUnix": 1724616654,
  "detail": "Applied NVIDIA GPU tuning: core +165 MHz, memory +420 MHz, voltage -35 mV, power limit 114%, temp limit 83°C."
}
```

---

### 8. ApplyFanProfile
**Request:**
```json
{
  "kind": "applyFanProfile",
  "payload": {
    "profileId": "auto"
  }
}
```

**Valid `profileId` values:**
- `"auto"`
- `"max"`
- `"custom"` (requires saved fan curves)

**Response:**
```json
{
  "profileId": "auto",
  "curves": null,
  "cpuSpeedPercent": 45,
  "gpuSpeedPercent": 40,
  "readback": null,
  "quietAutoFanCalibration": {
    "cpu": { "lastPercent": 18, "lastRpm": 2100, "idlePercent": 18, "elevatedPercent": 24 },
    "gpu": { "lastPercent": 16, "lastRpm": 1950, "idlePercent": 16, "elevatedPercent": 22 },
    "lastTargetRpm": 2000,
    "updatedAtUnix": 1724616654
  },
  "quietAutoThermalWarning": null,
  "appliedAtUnix": 1724616654,
  "detail": "Applied auto fan profile."
}
```

---

### 9. ApplyCustomFanCurves
**Request:**
```json
{
  "kind": "applyCustomFanCurves",
  "payload": {
    "curves": {
      "cpu": [
        { "tempC": 30, "speedPercent": 2 },
        { "tempC": 49, "speedPercent": 2 },
        { "tempC": 65, "speedPercent": 22 },
        { "tempC": 74, "speedPercent": 64 },
        { "tempC": 80, "speedPercent": 100 }
      ],
      "gpu": [
        { "tempC": 30, "speedPercent": 2 },
        { "tempC": 49, "speedPercent": 2 },
        { "tempC": 65, "speedPercent": 22 },
        { "tempC": 74, "speedPercent": 64 },
        { "tempC": 80, "speedPercent": 100 }
      ]
    },
    "quietSuccessLog": false
  }
}
```

**Curve constraints:**
- Exactly **5 points** per fan (CPU and GPU)
- Temperature range: 30°C - 90°C
- Speed range: 0% - 100%
- Points must be sorted by ascending temperature

**Response:**
```json
{
  "profileId": "custom",
  "curves": {
    "cpu": [ /* 5 points */ ],
    "gpu": [ /* 5 points */ ]
  },
  "cpuSpeedPercent": 22,
  "gpuSpeedPercent": 18,
  "readback": null,
  "quietAutoFanCalibration": null,
  "quietAutoThermalWarning": null,
  "appliedAtUnix": 1724616654,
  "detail": "Applied custom fan curves: CPU 22%, GPU 18% at current temps."
}
```

---

### 10. StartFanSpeedCalibration
**Request:**
```json
{ "kind": "startFanSpeedCalibration" }
```

**Response:**
```json
{
  "running": true,
  "status": "Fan speed calibration queued. Testing every 5% with a 20 second settle per step.",
  "startedAtUnix": 1724616654,
  "updatedAtUnix": 1724616654,
  "completedAtUnix": null,
  "currentPercent": null,
  "settleSeconds": 20,
  "points": [],
  "lastError": null
}
```

---

### 11. CancelFanSpeedCalibration
**Request:**
```json
{ "kind": "cancelFanSpeedCalibration" }
```

**Response:**
```json
{
  "running": false,
  "status": "Fan speed calibration cancel requested. Restoring the previous fan mode.",
  "startedAtUnix": 1724616600,
  "updatedAtUnix": 1724616654,
  "completedAtUnix": 1724616654,
  "currentPercent": 45,
  "settleSeconds": 20,
  "points": [ /* collected calibration points */ ],
  "lastError": null
}
```

---

### 12. ApplyBootLogo
**Request:**
```json
{
  "kind": "applyBootLogo",
  "payload": {
    "imagePath": "C:\\ProgramData\\AeroForge\\boot-logos\\ember.png",
    "originalFilename": "ember.png"
  }
}
```

**Response:**
```json
{
  "imagePath": "C:\\ProgramData\\AeroForge\\boot-logos\\ember.png",
  "originalFilename": "ember.png",
  "readback": null,
  "appliedAtUnix": 1724616654,
  "detail": "Boot logo flashed to EFI partition successfully."
}
```

---

### 13. ApplySmartCharging
**Request:**
```json
{
  "kind": "applySmartCharging",
  "payload": {
    "enabled": true
  }
}
```

**Response:**
```json
{
  "enabled": true,
  "healthStatus": 100,
  "batteryHealthy": 80,
  "appliedAtUnix": 1724616654,
  "detail": "Smart charging enabled. Battery will charge to 80% maximum."
}
```

---

### 14. ApplyTelemetrySettings
**Request:**
```json
{
  "kind": "applyTelemetrySettings",
  "payload": {
    "nvidiaTelemetryEnabled": true
  }
}
```

**Response:**
```json
{
  "nvidiaTelemetryEnabled": true,
  "detail": "NVIDIA telemetry polling enabled. AeroForge may read dGPU clocks, power, and limits when Windows reports active dGPU memory."
}
```

---

## Cached State Files

The service writes snapshots to `C:\ProgramData\AeroForge\Service\state\`:

- **`supervisor.json`** - Service worker health (used for fast polling)
- **`telemetry.json`** - Last telemetry snapshot
- **`control.json`** - Last control state snapshot
- **`capabilities.json`** - Hardware capability snapshot

The Tauri frontend uses these cached files for fast UI updates (1000ms polling) without blocking on the named pipe.

---

## Error Handling

- **Pipe unavailable (OS error 2):** Service not installed or not running
- **Pipe timeout:** Service busy or hung (retry with backoff)
- **Worker degraded:** `critical_worker_problem()` detected stale heartbeat or error state
- **Command errors:** Returned as `{ "kind": "error", "message": "..." }`

---

## Connection Recovery

1. Try pipe connection with 20 retries, 100ms intervals
2. If 3+ failures and `should_try_service_recovery()` triggers:
   - Check `supervisor.json` for worker problems
   - Query service state via `sc.exe query AeroForgeService`
   - Restart service if RUNNING but unhealthy, or start if STOPPED
3. Fall back to cached snapshots if pipe unavailable

---

## Polling Strategy (Frontend)

- **Active window:** Poll every **1000ms** via `get_backend_poll_snapshot`
- **Minimized/hidden:** Poll every **5000ms**
- **Debounce slider inputs:** ~150ms before sending IPC commands
- **Auto-reconnect:** Backoff on pipe errors, surface connection status in UI
