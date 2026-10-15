import { describe, expect, it } from 'vitest'
import type { ControlSnapshot } from './backend'
import {
  SETTINGS_EXPORT_FORMAT,
  SETTINGS_EXPORT_VERSION,
  buildPortableSettings,
  defaultSettingsExportFilename,
  parsePortableSettings,
  serializePortableSettings,
} from './settingsPortable'

const controls: ControlSnapshot = {
  activePowerProfile: 'balanced',
  activeFanProfile: 'auto',
  customProcessorState: { minPercent: 35, maxPercent: 88 },
  customPowerBase: 'performance',
  gpuTuning: {
    coreClockMhz: 0,
    memoryClockMhz: 0,
    voltageOffsetMv: 0,
    powerLimitPercent: 100,
    tempLimitC: 83,
  },
  ocPresets: [],
  activeOcSlot: 'daily',
  ocApplyState: 'live',
  ocTuningLocked: false,
  fanCurves: {
    cpu: [
      { tempC: 30, speedPercent: 0 },
      { tempC: 90, speedPercent: 100 },
    ],
    gpu: [
      { tempC: 30, speedPercent: 0 },
      { tempC: 90, speedPercent: 100 },
    ],
  },
  fanPresets: [],
  powerPresets: [],
  quickPresets: [],
  fanSyncLockEnabled: false,
  personalSettings: {
    smartChargingEnabled: true,
    usbPowerEnabled: true,
    processorStateControlEnabled: true,
    nvidiaTelemetryEnabled: false,
    keepUiPrewarmed: false,
    blueLightFilterEnabled: false,
    autoRefreshRateOnBatteryEnabled: false,
    autoRefreshRateRestoreHz: null,
    autoPerformanceEnabled: false,
    selectedBootArt: 'ember',
    customBootFilename: 'custom-boot.png',
    updateChannel: 'stable',
    checkForUpdatesOnLaunch: true,
  },
}

function withControls(patch: Partial<Record<keyof ControlSnapshot, unknown>>) {
  return serializePortableSettings({
    ...buildPortableSettings(controls, '0.15.6'),
    controls: { ...controls, ...patch } as ControlSnapshot,
  })
}

describe('buildPortableSettings / serializePortableSettings', () => {
  it('wraps the snapshot with format, version and a unix timestamp', () => {
    const payload = buildPortableSettings(controls, '0.15.6')
    expect(payload.format).toBe(SETTINGS_EXPORT_FORMAT)
    expect(payload.version).toBe(SETTINGS_EXPORT_VERSION)
    expect(payload.appVersion).toBe('0.15.6')
    expect(Number.isInteger(payload.exportedAtUnix)).toBe(true)
    expect(payload.controls).toBe(controls)
  })

  it('serializes as pretty JSON with a trailing newline', () => {
    const text = serializePortableSettings(buildPortableSettings(controls))
    expect(text.endsWith('}\n')).toBe(true)
    expect(text).toContain('\n  "format"')
  })
})

describe('parsePortableSettings', () => {
  it('round-trips an exported payload', () => {
    const payload = buildPortableSettings(controls, '0.15.6')
    const result = parsePortableSettings(serializePortableSettings(payload))
    expect(result.ok).toBe(true)
    if (result.ok) {
      expect(result.payload.controls).toEqual(controls)
      expect(result.payload.appVersion).toBe('0.15.6')
      expect(result.payload.exportedAtUnix).toBe(payload.exportedAtUnix)
    }
  })

  it('reports invalid JSON and non-object roots', () => {
    expect(parsePortableSettings('nope')).toEqual({
      ok: false,
      error: 'Settings file is not valid JSON.',
    })
    expect(parsePortableSettings('[1,2]')).toMatchObject({ ok: false })
  })

  it('rejects a foreign format and names it in the error', () => {
    const result = parsePortableSettings(JSON.stringify({ format: 'other-tool', version: 1 }))
    expect(result.ok).toBe(false)
    if (!result.ok) {
      expect(result.error).toContain('other-tool')
      expect(result.error).toContain(SETTINGS_EXPORT_FORMAT)
    }
  })

  it('rejects an unsupported version', () => {
    const result = parsePortableSettings(
      JSON.stringify({ format: SETTINGS_EXPORT_FORMAT, version: 2, controls }),
    )
    expect(result).toMatchObject({ ok: false })
    if (!result.ok) {
      expect(result.error).toContain('version 2')
    }
  })

  it.each([
    ['activePowerProfile', 'ludicrous'],
    ['activeFanProfile', 'off'],
    ['customPowerBase', 'battery-guard'],
    ['customProcessorState', { minPercent: 90, maxPercent: 40 }],
    ['customProcessorState', { minPercent: -1, maxPercent: 50 }],
    ['gpuTuning', { coreClockMhz: 'fast' }],
    ['fanCurves', { cpu: [{ tempC: 30 }], gpu: [] }],
    ['fanPresets', 'not-an-array'],
    ['ocPresets', null],
    ['activeOcSlot', 3],
    ['ocApplyState', 'pending'],
    ['personalSettings', null],
    ['fanSyncLockEnabled', 'yes'],
  ])('rejects a controls snapshot with a bad %s', (key, value) => {
    const result = parsePortableSettings(withControls({ [key]: value }))
    expect(result.ok).toBe(false)
    if (!result.ok) {
      expect(result.error).toContain('controls snapshot')
    }
  })

  it('drops malformed presets instead of letting them poison later saves', () => {
    const result = parsePortableSettings(
      withControls({
        fanPresets: [
          { id: 'ok', name: 'Quiet', profileId: 'auto', curves: controls.fanCurves },
          { id: 5, profileId: 'nope' },
        ],
        powerPresets: [{ id: 'p', name: 'x', profileId: 'turbo', customBase: 'nope' }],
        quickPresets: [{ id: 'q', name: 'Combo', fanPresetId: 'ok', powerPresetId: 'p' }],
        ocPresets: [{ id: 'daily', isCustom: false }],
      }),
    )
    expect(result.ok).toBe(true)
    if (result.ok) {
      expect(result.payload.controls.fanPresets.map((preset) => preset.id)).toEqual(['ok'])
      expect(result.payload.controls.powerPresets).toEqual([])
      expect(result.payload.controls.quickPresets).toEqual([
        { id: 'q', name: 'Combo', fanPresetId: 'ok', powerPresetId: null },
      ])
      expect(result.payload.controls.ocPresets).toEqual([])
      expect(result.warnings).toHaveLength(3)
    }
  })

  it('normalizes personal settings to the backend defaults', () => {
    const result = parsePortableSettings(
      withControls({
        personalSettings: {
          smartChargingEnabled: 'yes',
          autoRefreshRateRestoreHz: '144',
          selectedBootArt: 'neon',
          updateChannel: 'nightly',
          customBootFilename: '',
          keepUiPrewarmed: true,
        },
      }),
    )
    expect(result.ok).toBe(true)
    if (result.ok) {
      expect(result.payload.controls.personalSettings).toEqual({
        ...controls.personalSettings,
        keepUiPrewarmed: true,
      })
      expect(result.warnings).toEqual([])
    }
  })

  it('fills in a timestamp and null version when the file omits them', () => {
    const result = parsePortableSettings(
      JSON.stringify({ format: SETTINGS_EXPORT_FORMAT, version: 1, controls }),
    )
    expect(result.ok).toBe(true)
    if (result.ok) {
      expect(result.payload.appVersion).toBeNull()
      expect(Number.isInteger(result.payload.exportedAtUnix)).toBe(true)
    }
  })
})

describe('defaultSettingsExportFilename', () => {
  it('embeds a sanitized version and a filesystem-safe timestamp', () => {
    const name = defaultSettingsExportFilename('0.15.6-beta/1')
    expect(name).toMatch(/^AeroForge-Settings-v0\.15\.6-beta_1-\d{4}-\d{2}-\d{2}-\d{2}-\d{2}-\d{2}\.json$/)
  })

  it('omits the version segment when none is known', () => {
    expect(defaultSettingsExportFilename(null)).toMatch(/^AeroForge-Settings-\d{4}-/)
  })
})
