import type { ControlSnapshot } from './backend'
import { duplicateCurveSet, type CurveSet } from './curves'

export type FanControlPreset = ControlSnapshot['fanPresets'][number]
export type PowerControlPreset = ControlSnapshot['powerPresets'][number]
export type QuickControlPreset = ControlSnapshot['quickPresets'][number]

export const PRESET_PACK_KIND = 'aeroforge-preset-pack' as const
export const PRESET_PACK_SCHEMA_VERSION = 1

export type PresetPack = {
  kind: typeof PRESET_PACK_KIND
  schemaVersion: number
  exportedAt: string
  appVersion: string
  fanPresets: FanControlPreset[]
  powerPresets: PowerControlPreset[]
  quickPresets: QuickControlPreset[]
}

export type ParsedPresetPack = {
  pack: PresetPack
  warnings: string[]
}

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function isCurvePoint(value: unknown): value is { tempC: number; speedPercent: number } {
  if (!isObject(value)) {
    return false
  }
  return (
    typeof value.tempC === 'number' &&
    Number.isFinite(value.tempC) &&
    typeof value.speedPercent === 'number' &&
    Number.isFinite(value.speedPercent)
  )
}

function sanitizeCurveSet(value: unknown): CurveSet | null {
  if (!isObject(value) || !Array.isArray(value.cpu) || !Array.isArray(value.gpu)) {
    return null
  }
  if (!value.cpu.every(isCurvePoint) || !value.gpu.every(isCurvePoint)) {
    return null
  }

  return duplicateCurveSet({
    cpu: value.cpu.map((point) => ({
      temp: Math.round(point.tempC),
      speed: Math.round(point.speedPercent),
    })),
    gpu: value.gpu.map((point) => ({
      temp: Math.round(point.tempC),
      speed: Math.round(point.speedPercent),
    })),
  })
}

function toBackendCurveSet(curves: CurveSet) {
  return {
    cpu: curves.cpu.map((point) => ({
      tempC: point.temp,
      speedPercent: point.speed,
    })),
    gpu: curves.gpu.map((point) => ({
      tempC: point.temp,
      speedPercent: point.speed,
    })),
  }
}

function sanitizeFanPreset(value: unknown, index: number): FanControlPreset | null {
  if (!isObject(value)) {
    return null
  }
  const profileId = value.profileId
  if (
    profileId !== 'auto' &&
    profileId !== 'cool' &&
    profileId !== 'max' &&
    profileId !== 'custom'
  ) {
    return null
  }
  const curves = sanitizeCurveSet(value.curves)
  if (!curves) {
    return null
  }
  const name =
    typeof value.name === 'string' && value.name.trim().length > 0
      ? value.name.trim().slice(0, 64)
      : `Imported Fan ${index + 1}`
  const id =
    typeof value.id === 'string' && value.id.trim().length > 0
      ? value.id.trim().slice(0, 64)
      : `fan-import-${Date.now()}-${index}`

  return {
    id,
    name,
    profileId,
    curves: toBackendCurveSet(curves),
  }
}

function sanitizePowerPreset(value: unknown, index: number): PowerControlPreset | null {
  if (!isObject(value)) {
    return null
  }
  const profileId = value.profileId
  if (
    profileId !== 'battery-guard' &&
    profileId !== 'balanced' &&
    profileId !== 'performance' &&
    profileId !== 'turbo' &&
    profileId !== 'custom'
  ) {
    return null
  }
  const customBase = value.customBase
  if (customBase !== 'balanced' && customBase !== 'performance' && customBase !== 'turbo') {
    return null
  }
  if (!isObject(value.processorState)) {
    return null
  }
  const min = Number(value.processorState.minPercent)
  const max = Number(value.processorState.maxPercent)
  if (!Number.isFinite(min) || !Number.isFinite(max)) {
    return null
  }
  const minPercent = Math.round(Math.min(100, Math.max(0, min)))
  const maxPercent = Math.round(Math.min(100, Math.max(5, max)))
  // The service rejects min > max at apply time; catching it here surfaces the
  // problem at import instead of as a confusing failure later.
  if (minPercent > maxPercent) {
    return null
  }

  const name =
    typeof value.name === 'string' && value.name.trim().length > 0
      ? value.name.trim().slice(0, 64)
      : `Imported Power ${index + 1}`
  const id =
    typeof value.id === 'string' && value.id.trim().length > 0
      ? value.id.trim().slice(0, 64)
      : `power-import-${Date.now()}-${index}`

  return {
    id,
    name,
    profileId,
    customBase,
    processorState: { minPercent, maxPercent },
  }
}

function sanitizeQuickPreset(
  value: unknown,
  index: number,
  fanIds: Set<string>,
  powerIds: Set<string>,
): QuickControlPreset | null {
  if (!isObject(value)) {
    return null
  }
  const fanPresetId =
    typeof value.fanPresetId === 'string' && fanIds.has(value.fanPresetId)
      ? value.fanPresetId
      : null
  const powerPresetId =
    typeof value.powerPresetId === 'string' && powerIds.has(value.powerPresetId)
      ? value.powerPresetId
      : null
  if (!fanPresetId && !powerPresetId) {
    return null
  }

  const name =
    typeof value.name === 'string' && value.name.trim().length > 0
      ? value.name.trim().slice(0, 64)
      : `Imported Combo ${index + 1}`
  const id =
    typeof value.id === 'string' && value.id.trim().length > 0
      ? value.id.trim().slice(0, 64)
      : `quick-import-${Date.now()}-${index}`

  return {
    id,
    name,
    fanPresetId,
    powerPresetId,
  }
}

export function buildPresetPack(input: {
  appVersion: string
  fanPresets: FanControlPreset[]
  powerPresets: PowerControlPreset[]
  quickPresets: QuickControlPreset[]
}): PresetPack {
  return {
    kind: PRESET_PACK_KIND,
    schemaVersion: PRESET_PACK_SCHEMA_VERSION,
    exportedAt: new Date().toISOString(),
    appVersion: input.appVersion,
    fanPresets: input.fanPresets,
    powerPresets: input.powerPresets,
    quickPresets: input.quickPresets,
  }
}

/**
 * Drops malformed presets and combos that point at nothing, so every importer
 * (preset packs and full settings backups) hands the backend the same shape it
 * would have produced itself.
 */
export function sanitizePresetCollections(
  fanSource: unknown[],
  powerSource: unknown[],
  quickSource: unknown[],
) {
  const warnings: string[] = []
  const fanPresets = fanSource
    .map((item, index) => sanitizeFanPreset(item, index))
    .filter((item): item is FanControlPreset => item !== null)
  const powerPresets = powerSource
    .map((item, index) => sanitizePowerPreset(item, index))
    .filter((item): item is PowerControlPreset => item !== null)

  if (fanSource.length !== fanPresets.length) {
    warnings.push(`Skipped ${fanSource.length - fanPresets.length} invalid fan preset(s).`)
  }
  if (powerSource.length !== powerPresets.length) {
    warnings.push(`Skipped ${powerSource.length - powerPresets.length} invalid power preset(s).`)
  }

  const fanIds = new Set(fanPresets.map((preset) => preset.id))
  const powerIds = new Set(powerPresets.map((preset) => preset.id))
  const quickPresets = quickSource
    .map((item, index) => sanitizeQuickPreset(item, index, fanIds, powerIds))
    .filter((item): item is QuickControlPreset => item !== null)

  if (quickSource.length !== quickPresets.length) {
    warnings.push(
      `Skipped ${quickSource.length - quickPresets.length} combo preset(s) with missing references.`,
    )
  }

  return { fanPresets, powerPresets, quickPresets, warnings }
}

export function parsePresetPack(raw: string): ParsedPresetPack {
  let parsed: unknown
  try {
    parsed = JSON.parse(raw)
  } catch {
    throw new Error('Preset file is not valid JSON.')
  }

  if (!isObject(parsed)) {
    throw new Error('Preset file root must be an object.')
  }

  if (parsed.kind !== PRESET_PACK_KIND) {
    throw new Error('This file is not an AeroForge preset pack.')
  }

  const schemaVersion = Number(parsed.schemaVersion ?? 1)
  if (!Number.isFinite(schemaVersion) || schemaVersion > PRESET_PACK_SCHEMA_VERSION) {
    throw new Error(`Unsupported preset pack schema version: ${parsed.schemaVersion}`)
  }

  const { fanPresets, powerPresets, quickPresets, warnings } = sanitizePresetCollections(
    Array.isArray(parsed.fanPresets) ? parsed.fanPresets : [],
    Array.isArray(parsed.powerPresets) ? parsed.powerPresets : [],
    Array.isArray(parsed.quickPresets) ? parsed.quickPresets : [],
  )

  if (fanPresets.length === 0 && powerPresets.length === 0 && quickPresets.length === 0) {
    throw new Error('Preset pack did not contain any usable presets.')
  }

  return {
    pack: {
      kind: PRESET_PACK_KIND,
      schemaVersion: PRESET_PACK_SCHEMA_VERSION,
      exportedAt:
        typeof parsed.exportedAt === 'string' ? parsed.exportedAt : new Date().toISOString(),
      appVersion: typeof parsed.appVersion === 'string' ? parsed.appVersion : 'unknown',
      fanPresets,
      powerPresets,
      quickPresets,
    },
    warnings,
  }
}

export function mergePresetPack(
  existing: {
    fanPresets: FanControlPreset[]
    powerPresets: PowerControlPreset[]
    quickPresets: QuickControlPreset[]
  },
  incoming: PresetPack,
  mode: 'replace' | 'merge',
) {
  if (mode === 'replace') {
    return {
      fanPresets: incoming.fanPresets,
      powerPresets: incoming.powerPresets,
      quickPresets: incoming.quickPresets,
    }
  }

  const stamp = Date.now().toString(36)
  const rekey = <T extends { id: string; name: string }>(
    items: T[],
    existingIds: Set<string>,
    prefix: string,
  ) =>
    items.map((item, index) => {
      if (!existingIds.has(item.id)) {
        existingIds.add(item.id)
        return item
      }
      const nextId = `${prefix}-${stamp}-${index}`
      existingIds.add(nextId)
      return {
        ...item,
        id: nextId,
        name: `${item.name} (imported)`,
      }
    })

  const fanIds = new Set(existing.fanPresets.map((item) => item.id))
  const powerIds = new Set(existing.powerPresets.map((item) => item.id))
  const quickIds = new Set(existing.quickPresets.map((item) => item.id))

  const fanIdMap = new Map<string, string>()
  const powerIdMap = new Map<string, string>()

  const mergedFans = rekey(incoming.fanPresets, fanIds, 'fan').map((item, index) => {
    const original = incoming.fanPresets[index]
    fanIdMap.set(original.id, item.id)
    return item
  })
  const mergedPower = rekey(incoming.powerPresets, powerIds, 'power').map((item, index) => {
    const original = incoming.powerPresets[index]
    powerIdMap.set(original.id, item.id)
    return item
  })
  const mergedQuick = rekey(
    incoming.quickPresets.map((preset) => ({
      ...preset,
      fanPresetId: preset.fanPresetId
        ? (fanIdMap.get(preset.fanPresetId) ?? preset.fanPresetId)
        : null,
      powerPresetId: preset.powerPresetId
        ? (powerIdMap.get(preset.powerPresetId) ?? preset.powerPresetId)
        : null,
    })),
    quickIds,
    'quick',
  )

  return {
    fanPresets: [...existing.fanPresets, ...mergedFans],
    powerPresets: [...existing.powerPresets, ...mergedPower],
    quickPresets: [...existing.quickPresets, ...mergedQuick],
  }
}
