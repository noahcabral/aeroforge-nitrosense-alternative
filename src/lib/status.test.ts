import { describe, expect, it } from 'vitest'
import type { ControlLayerStatus } from './backend'
import { buildCpuThermalSummary, describeFeatureSupport, summarizeLayerAttention } from './status'

describe('status text', () => {
  it('lists only layers that need attention', () => {
    const layers: ControlLayerStatus[] = [
      { id: 'a', label: 'WMI', state: 'applied', detail: '' },
      { id: 'b', label: 'RAPL', state: 'failed', detail: '' },
      { id: 'c', label: 'NVAPI', state: 'unavailable', detail: '' },
    ]
    expect(summarizeLayerAttention(layers)).toBe('RAPL: Failed | NVAPI: Unavailable')
    expect(summarizeLayerAttention(layers.slice(0, 1))).toBeNull()
    expect(summarizeLayerAttention(null)).toBeNull()
  })

  it('explains unsupported and read-only features, and stays quiet when writable', () => {
    expect(describeFeatureSupport(null, 'no')).toBeNull()
    expect(describeFeatureSupport({ available: false, writable: false } as never, 'no', 'staged')).toBe('no')
    expect(describeFeatureSupport({ available: true, writable: false } as never, 'no', 'staged')).toBe('staged')
    expect(describeFeatureSupport({ available: true, writable: true } as never, 'no')).toBeNull()
  })

  it('joins only the thermal parts that are known', () => {
    expect(buildCpuThermalSummary(40, 72, 15)).toBe('Low 40C / High 72C • Usage 15%')
    expect(buildCpuThermalSummary(null, 72, null)).toBe('')
  })
})
