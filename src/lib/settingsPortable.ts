/**
 * Portable export/import of AeroForge desktop control state.
 * Safe for local backup and restore; does not include secrets or update tokens.
 */

import type {
  ControlSnapshot,
  CustomPowerBaseId,
  FanCurveSet,
  FanProfileId,
  GpuTuningState,
  PowerProfileId,
  ProcessorStateSettings,
} from './backend'

export const SETTINGS_EXPORT_FORMAT = 'aeroforge-control-settings' as const
export const SETTINGS_EXPORT_VERSION = 1 as const

export type PortableSettingsPayload = {
  format: typeof SETTINGS_EXPORT_FORMAT
  version: typeof SETTINGS_EXPORT_VERSION
  exportedAtUnix: number
  appVersion: string | null
  controls: ControlSnapshot
}

export type PortableSettingsParseResult =
  | { ok: true; payload: PortableSettingsPayload }
  | { ok: false; error: string }

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function isPowerProfileId(value: unknown): value is PowerProfileId {
  return (
    value === 'battery-guard' ||
    value === 'balanced' ||
    value === 'performance' ||
    value === 'turbo' ||
    value === 'custom'
  )
}

function isFanProfileId(value: unknown): value is FanProfileId {
  return value === 'auto' || value === 'cool' || value === 'max' || value === 'custom'
}

function isCustomPowerBaseId(value: unknown): value is CustomPowerBaseId {
  return value === 'balanced' || value === 'performance' || value === 'turbo'
}

function isProcessorState(value: unknown): value is ProcessorStateSettings {
  if (!isObject(value)) {
    return false
  }
  return (
    typeof value.minPercent === 'number' &&
    typeof value.maxPercent === 'number' &&
    value.minPercent >= 0 &&
    value.maxPercent <= 100 &&
    value.minPercent <= value.maxPercent
  )
}

function isFanCurveSet(value: unknown): value is FanCurveSet {
  if (!isObject(value) || !Array.isArray(value.cpu) || !Array.isArray(value.gpu)) {
    return false
  }
  const validPoint = (point: unknown) =>
    isObject(point) &&
    typeof point.tempC === 'number' &&
    typeof point.speedPercent === 'number'
  return value.cpu.every(validPoint) && value.gpu.every(validPoint)
}

function isGpuTuning(value: unknown): value is GpuTuningState {
  if (!isObject(value)) {
    return false
  }
  return (
    typeof value.coreClockMhz === 'number' &&
    typeof value.memoryClockMhz === 'number' &&
    typeof value.voltageOffsetMv === 'number' &&
    typeof value.powerLimitPercent === 'number' &&
    typeof value.tempLimitC === 'number'
  )
}

function isControlSnapshot(value: unknown): value is ControlSnapshot {
  if (!isObject(value)) {
    return false
  }
  return (
    isPowerProfileId(value.activePowerProfile) &&
    isFanProfileId(value.activeFanProfile) &&
    isProcessorState(value.customProcessorState) &&
    isCustomPowerBaseId(value.customPowerBase) &&
    isGpuTuning(value.gpuTuning) &&
    isFanCurveSet(value.fanCurves) &&
    Array.isArray(value.fanPresets) &&
    Array.isArray(value.powerPresets) &&
    Array.isArray(value.quickPresets) &&
    Array.isArray(value.ocPresets) &&
    typeof value.fanSyncLockEnabled === 'boolean' &&
    isObject(value.personalSettings)
  )
}

export function buildPortableSettings(
  controls: ControlSnapshot,
  appVersion: string | null = null,
): PortableSettingsPayload {
  return {
    format: SETTINGS_EXPORT_FORMAT,
    version: SETTINGS_EXPORT_VERSION,
    exportedAtUnix: Math.floor(Date.now() / 1000),
    appVersion,
    controls,
  }
}

export function serializePortableSettings(payload: PortableSettingsPayload): string {
  return `${JSON.stringify(payload, null, 2)}\n`
}

export function parsePortableSettings(raw: string): PortableSettingsParseResult {
  let parsed: unknown
  try {
    parsed = JSON.parse(raw)
  } catch {
    return { ok: false, error: 'Settings file is not valid JSON.' }
  }

  if (!isObject(parsed)) {
    return { ok: false, error: 'Settings file root must be a JSON object.' }
  }

  if (parsed.format !== SETTINGS_EXPORT_FORMAT) {
    return {
      ok: false,
      error: `Unrecognized settings format${
        typeof parsed.format === 'string' ? ` (${parsed.format})` : ''
      }. Expected ${SETTINGS_EXPORT_FORMAT}.`,
    }
  }

  if (parsed.version !== SETTINGS_EXPORT_VERSION) {
    return {
      ok: false,
      error: `Unsupported settings version ${String(parsed.version)}. This build supports v${SETTINGS_EXPORT_VERSION}.`,
    }
  }

  if (!isControlSnapshot(parsed.controls)) {
    return {
      ok: false,
      error: 'Settings file is missing a valid controls snapshot (profiles, curves, or presets).',
    }
  }

  return {
    ok: true,
    payload: {
      format: SETTINGS_EXPORT_FORMAT,
      version: SETTINGS_EXPORT_VERSION,
      exportedAtUnix:
        typeof parsed.exportedAtUnix === 'number'
          ? parsed.exportedAtUnix
          : Math.floor(Date.now() / 1000),
      appVersion: typeof parsed.appVersion === 'string' ? parsed.appVersion : null,
      controls: parsed.controls,
    },
  }
}

export function defaultSettingsExportFilename(version: string | null): string {
  const stamp = new Date().toISOString().slice(0, 19).replace(/[:T]/g, '-')
  const versionPart = version ? `v${version.replace(/[^\w.-]+/g, '_')}-` : ''
  return `AeroForge-Settings-${versionPart}${stamp}.json`
}
