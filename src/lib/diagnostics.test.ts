import { describe, expect, it } from 'vitest'
import type { LiveControlSnapshot } from './backend'
import { formatDiagnosticsText, type DiagnosticsBundle } from './diagnostics'

const bundle: DiagnosticsBundle = {
  generatedAt: '2026-01-01T00:00:00.000Z',
  appVersion: '1.2.3',
  shellStatus: 'tauri v1.2.3',
  statusMessage: 'Ready',
  serviceConnected: false,
  serviceStatus: null,
  hardwareReadiness: null,
  capabilities: null,
  activePowerProfile: 'balanced',
  activeFanProfile: 'auto',
  processorState: { min: 5, max: 100 },
  customPowerBase: 'balanced',
  processorStateControlEnabled: true,
  smartChargingEnabled: false,
  nvidiaTelemetryEnabled: true,
  blueLightFilterEnabled: false,
  telemetry: null,
  liveControls: null,
  lastBackendError: null,
  lastBackendPollAt: 'Waiting',
  telemetrySourceLabel: 'No telemetry',
  recentEvents: [],
  frameStats: {
    averageMs: 4.26,
    maxMs: 17.9,
    fps: 59.6,
    longFrameCount: 1,
    sampleWindowMs: 1000,
    updatedAt: '12:00:00',
  },
  performanceLogPath: null,
}

describe('formatDiagnosticsText', () => {
  it('skips every optional section when nothing is loaded', () => {
    const text = formatDiagnosticsText(bundle)

    expect(text).toContain('Service connected: no')
    expect(text).toContain('Processor policy: on (5-100%) base=balanced')
    expect(text).toContain('Frame avg 4.3ms max 17.9ms fps 60 long 1 @ 12:00:00')
    expect(text).not.toContain('Last backend error')
    expect(text).not.toMatch(/--- (Service|Hardware readiness|Capabilities|Telemetry|Live control|Recent debug) /)
    expect(text.endsWith('--- End ---')).toBe(true)
  })

  it('lists control layers and errors from the live snapshot', () => {
    const liveControls = {
      lastApplyDetail: '',
      lastError: 'WMI timed out',
      lastPowerLayerStatuses: [{ id: 'wmi', label: 'WMI', state: 'failed', detail: 'timeout' }],
      lastFanApplyDetail: 'Applied auto',
      lastFanError: null,
      lastFanLayerStatuses: [],
    } as unknown as LiveControlSnapshot
    const text = formatDiagnosticsText({
      ...bundle,
      lastBackendError: 'pipe closed',
      liveControls,
      recentEvents: ['12:00:01 boot'],
    })

    expect(text).toContain('Last backend error: pipe closed')
    expect(text).toContain('Power detail: (none)')
    expect(text).toContain('Power error: WMI timed out')
    expect(text).toContain('  power/wmi: failed — timeout')
    expect(text).toContain('Fan detail: Applied auto')
    expect(text).not.toContain('Fan error')
    expect(text).toContain('* 12:00:01 boot')
  })
})
