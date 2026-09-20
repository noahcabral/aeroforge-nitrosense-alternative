/**
 * Automatic Performance policy for AeroForge.
 *
 * Safety model (deliberately conservative):
 * - Opt-in only (caller must gate on the personal setting).
 * - Never recommends GPU clock offsets on battery.
 * - Only recommends NitroSense-matched stock offsets already shipped in the app
 *   (+100 core / +200 mem), never voltage, power %, or temp-limit writes.
 * - Backs off to Quiet/stock when thermals climb; refuses OC above a cool ceiling.
 * - Uses hysteresis so load flicker does not flap profiles.
 */

import type { PowerProfileId } from './backend'

export type AutoOcSlotId = 'silent-uv' | 'daily' | 'creator' | 'arena'

export type AutoPerformanceMode = 'quiet' | 'balanced' | 'performance' | 'turbo'

export type AutoPerformanceInput = {
  acPluggedIn: boolean
  cpuUsagePercent: number
  gpuUsagePercent: number
  cpuTempC: number
  gpuTempC: number
  systemTempC: number
  /** Sticky mode from the previous decision; null on first evaluation. */
  stickyMode: AutoPerformanceMode | null
  /** When true, policy may still change power mode but keeps OC at stock. */
  ocTuningLocked: boolean
  /** Live GPU offset apply requires NVIDIA telemetry / Whisper path opt-in. */
  nvidiaTelemetryEnabled: boolean
}

export type AutoPerformanceDecision = {
  mode: AutoPerformanceMode
  powerProfileId: Exclude<PowerProfileId, 'custom'>
  ocSlotId: AutoOcSlotId
  /** True when the decision wants live NitroSense Performance/Turbo offsets. */
  wantsGpuOffsets: boolean
  reason: string
}

/** Hottest package/sensor reading used for thermal backoff. */
export function hottestTempC(input: Pick<
  AutoPerformanceInput,
  'cpuTempC' | 'gpuTempC' | 'systemTempC'
>): number {
  return Math.max(input.cpuTempC, input.gpuTempC, input.systemTempC)
}

/** Peak of CPU/GPU utilization for load banding. */
export function peakLoadPercent(input: Pick<
  AutoPerformanceInput,
  'cpuUsagePercent' | 'gpuUsagePercent'
>): number {
  return Math.max(input.cpuUsagePercent, input.gpuUsagePercent)
}

const THERMAL_EMERGENCY_C = 90
const THERMAL_BACKOFF_C = 85
const THERMAL_OC_MAX_C = 82
const LOAD_TURBO_ENTER = 92
const LOAD_TURBO_EXIT = 78
const LOAD_PERFORMANCE_ENTER = 70
const LOAD_PERFORMANCE_EXIT = 48
const LOAD_BALANCED_ENTER = 28
const LOAD_QUIET_EXIT = 18

function modeFromLoad(load: number, sticky: AutoPerformanceMode | null): AutoPerformanceMode {
  if (sticky === 'turbo') {
    if (load >= LOAD_TURBO_EXIT) {
      return 'turbo'
    }
    if (load >= LOAD_PERFORMANCE_EXIT) {
      return 'performance'
    }
    if (load >= LOAD_QUIET_EXIT) {
      return 'balanced'
    }
    return 'quiet'
  }

  if (sticky === 'performance') {
    if (load >= LOAD_TURBO_ENTER) {
      return 'turbo'
    }
    if (load >= LOAD_PERFORMANCE_EXIT) {
      return 'performance'
    }
    if (load >= LOAD_QUIET_EXIT) {
      return 'balanced'
    }
    return 'quiet'
  }

  if (sticky === 'balanced') {
    if (load >= LOAD_TURBO_ENTER) {
      return 'turbo'
    }
    if (load >= LOAD_PERFORMANCE_ENTER) {
      return 'performance'
    }
    if (load >= LOAD_QUIET_EXIT) {
      return 'balanced'
    }
    return 'quiet'
  }

  // sticky quiet or first decision
  if (load >= LOAD_TURBO_ENTER) {
    return 'turbo'
  }
  if (load >= LOAD_PERFORMANCE_ENTER) {
    return 'performance'
  }
  if (load >= LOAD_BALANCED_ENTER) {
    return 'balanced'
  }
  return 'quiet'
}

function decisionForMode(
  mode: AutoPerformanceMode,
  options: {
    wantsGpuOffsets: boolean
    reason: string
  },
): AutoPerformanceDecision {
  switch (mode) {
    case 'quiet':
      return {
        mode,
        powerProfileId: 'battery-guard',
        ocSlotId: 'silent-uv',
        wantsGpuOffsets: false,
        reason: options.reason,
      }
    case 'balanced':
      return {
        mode,
        powerProfileId: 'balanced',
        ocSlotId: 'daily',
        wantsGpuOffsets: false,
        reason: options.reason,
      }
    case 'performance':
      return {
        mode,
        powerProfileId: 'performance',
        ocSlotId: options.wantsGpuOffsets ? 'creator' : 'daily',
        wantsGpuOffsets: options.wantsGpuOffsets,
        reason: options.reason,
      }
    case 'turbo':
      return {
        mode,
        powerProfileId: 'turbo',
        ocSlotId: options.wantsGpuOffsets ? 'arena' : 'daily',
        wantsGpuOffsets: options.wantsGpuOffsets,
        reason: options.reason,
      }
  }
}

/**
 * Decide the next Automatic Performance target from live telemetry.
 * Pure function: no I/O, safe to unit-test exhaustively.
 */
export function decideAutoPerformance(input: AutoPerformanceInput): AutoPerformanceDecision {
  const hottest = hottestTempC(input)
  const load = peakLoadPercent(input)

  if (!input.acPluggedIn) {
    return decisionForMode('quiet', {
      wantsGpuOffsets: false,
      reason: 'On battery: Quiet power with stock GPU clocks (no offsets).',
    })
  }

  if (hottest >= THERMAL_EMERGENCY_C) {
    return decisionForMode('quiet', {
      wantsGpuOffsets: false,
      reason: `Thermal emergency (${Math.round(hottest)}°C): Quiet + stock clocks.`,
    })
  }

  if (hottest >= THERMAL_BACKOFF_C) {
    return decisionForMode('balanced', {
      wantsGpuOffsets: false,
      reason: `Thermal backoff (${Math.round(hottest)}°C): Balanced + stock clocks.`,
    })
  }

  const mode = modeFromLoad(load, input.stickyMode)
  const coolEnoughForOc = hottest < THERMAL_OC_MAX_C
  const wantsGpuOffsets =
    coolEnoughForOc &&
    !input.ocTuningLocked &&
    input.nvidiaTelemetryEnabled &&
    (mode === 'performance' || mode === 'turbo')

  if (mode === 'turbo') {
    return decisionForMode('turbo', {
      wantsGpuOffsets,
      reason: wantsGpuOffsets
        ? `AC + high load (${Math.round(load)}%) and cool thermals: Turbo + NitroSense GPU offsets.`
        : coolEnoughForOc
          ? `AC + high load (${Math.round(load)}%): Turbo power; GPU offsets skipped (${
              input.ocTuningLocked
                ? 'tuner locked'
                : input.nvidiaTelemetryEnabled
                  ? 'stock clocks'
                  : 'NVIDIA telemetry off'
            }).`
          : `AC + high load (${Math.round(load)}%) but warm (${Math.round(hottest)}°C): Turbo without GPU offsets.`,
    })
  }

  if (mode === 'performance') {
    return decisionForMode('performance', {
      wantsGpuOffsets,
      reason: wantsGpuOffsets
        ? `AC + sustained load (${Math.round(load)}%) and cool thermals: Performance + NitroSense GPU offsets.`
        : coolEnoughForOc
          ? `AC + sustained load (${Math.round(load)}%): Performance power; GPU offsets skipped (${
              input.ocTuningLocked
                ? 'tuner locked'
                : input.nvidiaTelemetryEnabled
                  ? 'stock clocks'
                  : 'NVIDIA telemetry off'
            }).`
          : `AC + sustained load (${Math.round(load)}%) but warm (${Math.round(hottest)}°C): Performance without GPU offsets.`,
    })
  }

  if (mode === 'balanced') {
    return decisionForMode('balanced', {
      wantsGpuOffsets: false,
      reason: `AC + moderate load (${Math.round(load)}%): Balanced + stock GPU clocks.`,
    })
  }

  return decisionForMode('quiet', {
    wantsGpuOffsets: false,
    reason: `AC + light load (${Math.round(load)}%): Quiet + stock GPU clocks.`,
  })
}

export function autoPerformanceModeLabel(mode: AutoPerformanceMode): string {
  switch (mode) {
    case 'quiet':
      return 'Quiet'
    case 'balanced':
      return 'Balanced'
    case 'performance':
      return 'Performance'
    case 'turbo':
      return 'Turbo'
  }
}

/** True when the live control state already matches the decision target. */
export function autoPerformanceAlreadyApplied(
  decision: AutoPerformanceDecision,
  current: {
    powerProfileId: PowerProfileId
    ocSlotId: string
    coreClockMhz: number
    memoryClockMhz: number
  },
): boolean {
  if (current.powerProfileId !== decision.powerProfileId) {
    return false
  }

  if (decision.wantsGpuOffsets) {
    return (
      current.ocSlotId === decision.ocSlotId &&
      current.coreClockMhz === 100 &&
      (current.memoryClockMhz === 200 || current.memoryClockMhz === 0)
    )
  }

  // Stock clocks: accept any stock OC slot and zero offsets.
  return current.coreClockMhz === 0 && current.memoryClockMhz === 0
}
