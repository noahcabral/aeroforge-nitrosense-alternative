import { describe, expect, it } from 'vitest'
import {
  PRESET_PACK_KIND,
  PRESET_PACK_SCHEMA_VERSION,
  buildPresetPack,
  mergePresetPack,
  parsePresetPack,
  type FanControlPreset,
  type PowerControlPreset,
  type QuickControlPreset,
} from './presetTransfer'

const fanPreset: FanControlPreset = {
  id: 'fan-1',
  name: 'Quiet Night',
  profileId: 'custom',
  curves: {
    cpu: [
      { tempC: 30, speedPercent: 0 },
      { tempC: 60, speedPercent: 40 },
      { tempC: 90, speedPercent: 100 },
    ],
    gpu: [
      { tempC: 30, speedPercent: 0 },
      { tempC: 60, speedPercent: 40 },
      { tempC: 90, speedPercent: 100 },
    ],
  },
}

const powerPreset: PowerControlPreset = {
  id: 'power-1',
  name: 'Desk',
  profileId: 'custom',
  customBase: 'performance',
  processorState: { minPercent: 20, maxPercent: 90 },
}

const quickPreset: QuickControlPreset = {
  id: 'quick-1',
  name: 'Desk Combo',
  fanPresetId: 'fan-1',
  powerPresetId: 'power-1',
}

function packJson(overrides: Record<string, unknown> = {}) {
  return JSON.stringify({
    ...buildPresetPack({
      appVersion: '0.15.6',
      fanPresets: [fanPreset],
      powerPresets: [powerPreset],
      quickPresets: [quickPreset],
    }),
    ...overrides,
  })
}

describe('buildPresetPack', () => {
  it('stamps kind, schema version and export time', () => {
    const pack = buildPresetPack({
      appVersion: '1.2.3',
      fanPresets: [],
      powerPresets: [],
      quickPresets: [],
    })
    expect(pack.kind).toBe(PRESET_PACK_KIND)
    expect(pack.schemaVersion).toBe(PRESET_PACK_SCHEMA_VERSION)
    expect(pack.appVersion).toBe('1.2.3')
    expect(() => new Date(pack.exportedAt).toISOString()).not.toThrow()
  })
})

describe('parsePresetPack', () => {
  it('round-trips a pack built by buildPresetPack', () => {
    const { pack, warnings } = parsePresetPack(packJson())
    expect(warnings).toEqual([])
    expect(pack.fanPresets).toEqual([fanPreset])
    expect(pack.powerPresets).toEqual([powerPreset])
    expect(pack.quickPresets).toEqual([quickPreset])
  })

  it('rejects malformed input with a clear message', () => {
    expect(() => parsePresetPack('{')).toThrow('not valid JSON')
    expect(() => parsePresetPack('[]')).toThrow('root must be an object')
    expect(() => parsePresetPack('{"kind":"something-else"}')).toThrow(
      'not an AeroForge preset pack',
    )
    expect(() => parsePresetPack(packJson({ schemaVersion: 99 }))).toThrow(
      'Unsupported preset pack schema version',
    )
  })

  it('rejects a pack with nothing usable in it', () => {
    expect(() =>
      parsePresetPack(packJson({ fanPresets: [], powerPresets: [], quickPresets: [] })),
    ).toThrow('did not contain any usable presets')
  })

  it('skips fan presets with an unknown profile or broken curves and warns', () => {
    const { pack, warnings } = parsePresetPack(
      packJson({
        fanPresets: [
          fanPreset,
          { ...fanPreset, id: 'bad-profile', profileId: 'turbo' },
          { ...fanPreset, id: 'bad-curve', curves: { cpu: [{ tempC: 'hot' }], gpu: [] } },
        ],
      }),
    )
    expect(pack.fanPresets.map((preset) => preset.id)).toEqual(['fan-1'])
    expect(warnings).toContain('Skipped 2 invalid fan preset(s).')
  })

  it('normalizes imported fan curves through the curve invariants', () => {
    const { pack } = parsePresetPack(
      packJson({
        fanPresets: [
          {
            ...fanPreset,
            curves: {
              cpu: [
                { tempC: 70.4, speedPercent: 60 },
                { tempC: 30, speedPercent: 120 },
              ],
              gpu: [{ tempC: 30, speedPercent: 0 }],
            },
          },
        ],
      }),
    )
    expect(pack.fanPresets[0].curves.cpu).toEqual([
      { tempC: 30, speedPercent: 100 },
      { tempC: 70, speedPercent: 100 },
    ])
  })

  it('fills in a name and id when they are missing', () => {
    const { pack } = parsePresetPack(
      packJson({
        fanPresets: [{ ...fanPreset, id: '', name: '   ' }],
        powerPresets: [{ ...powerPreset, id: undefined, name: undefined }],
        quickPresets: [],
      }),
    )
    expect(pack.fanPresets[0].name).toBe('Imported Fan 1')
    expect(pack.fanPresets[0].id).toMatch(/^fan-import-/)
    expect(pack.powerPresets[0].name).toBe('Imported Power 1')
    expect(pack.powerPresets[0].id).toMatch(/^power-import-/)
  })

  it('truncates over-long names and ids', () => {
    const { pack } = parsePresetPack(
      packJson({ fanPresets: [{ ...fanPreset, id: 'x'.repeat(100), name: 'y'.repeat(100) }] }),
    )
    expect(pack.fanPresets[0].id).toHaveLength(64)
    expect(pack.fanPresets[0].name).toHaveLength(64)
  })

  it('clamps processor state into the range the service accepts', () => {
    const { pack } = parsePresetPack(
      packJson({
        powerPresets: [
          { ...powerPreset, processorState: { minPercent: -20, maxPercent: 140 } },
        ],
      }),
    )
    expect(pack.powerPresets[0].processorState).toEqual({ minPercent: 0, maxPercent: 100 })
  })

  it('skips a power preset whose minimum exceeds its maximum instead of importing it', () => {
    const { pack, warnings } = parsePresetPack(
      packJson({
        powerPresets: [
          powerPreset,
          { ...powerPreset, id: 'inverted', processorState: { minPercent: 90, maxPercent: 40 } },
        ],
      }),
    )
    expect(pack.powerPresets.map((preset) => preset.id)).toEqual(['power-1'])
    expect(warnings).toContain('Skipped 1 invalid power preset(s).')
  })

  it('drops combo presets whose references were not imported', () => {
    const { pack, warnings } = parsePresetPack(
      packJson({
        quickPresets: [
          quickPreset,
          { id: 'orphan', name: 'Orphan', fanPresetId: 'nope', powerPresetId: 'nope' },
        ],
      }),
    )
    expect(pack.quickPresets.map((preset) => preset.id)).toEqual(['quick-1'])
    expect(warnings).toContain('Skipped 1 combo preset(s) with missing references.')
  })

  it('keeps a combo that references only one side', () => {
    const { pack } = parsePresetPack(
      packJson({
        quickPresets: [
          { id: 'fan-only', name: 'Fan only', fanPresetId: 'fan-1', powerPresetId: 'nope' },
        ],
      }),
    )
    expect(pack.quickPresets).toEqual([
      { id: 'fan-only', name: 'Fan only', fanPresetId: 'fan-1', powerPresetId: null },
    ])
  })
})

describe('mergePresetPack', () => {
  const incoming = buildPresetPack({
    appVersion: '0.15.6',
    fanPresets: [fanPreset],
    powerPresets: [powerPreset],
    quickPresets: [quickPreset],
  })

  it('replace mode discards existing presets', () => {
    const merged = mergePresetPack(
      {
        fanPresets: [{ ...fanPreset, id: 'old' }],
        powerPresets: [],
        quickPresets: [],
      },
      incoming,
      'replace',
    )
    expect(merged.fanPresets).toEqual([fanPreset])
  })

  it('merge mode appends and keeps ids that do not collide', () => {
    const merged = mergePresetPack(
      { fanPresets: [], powerPresets: [], quickPresets: [] },
      incoming,
      'merge',
    )
    expect(merged.fanPresets).toEqual([fanPreset])
    expect(merged.quickPresets[0].fanPresetId).toBe('fan-1')
  })

  it('merge mode rekeys colliding presets and rewires combo references', () => {
    const merged = mergePresetPack(
      {
        fanPresets: [{ ...fanPreset, name: 'Existing Fan' }],
        powerPresets: [{ ...powerPreset, name: 'Existing Power' }],
        quickPresets: [{ ...quickPreset, name: 'Existing Combo' }],
      },
      incoming,
      'merge',
    )

    expect(merged.fanPresets).toHaveLength(2)
    expect(merged.fanPresets[0].name).toBe('Existing Fan')
    const importedFan = merged.fanPresets[1]
    expect(importedFan.id).not.toBe('fan-1')
    expect(importedFan.name).toBe('Quiet Night (imported)')

    const importedPower = merged.powerPresets[1]
    expect(importedPower.id).not.toBe('power-1')

    const importedQuick = merged.quickPresets[1]
    expect(importedQuick.id).not.toBe('quick-1')
    expect(importedQuick.fanPresetId).toBe(importedFan.id)
    expect(importedQuick.powerPresetId).toBe(importedPower.id)
  })
})
