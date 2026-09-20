import { describe, expect, it } from 'vitest'
import {
  findBestGpuOverclock,
  resultForTier,
  stepDownGpuOverclockTier,
  type AutoGpuFinderInput,
} from './autoGpuOverclock'

function base(overrides: Partial<AutoGpuFinderInput> = {}): AutoGpuFinderInput {
  return {
    acPluggedIn: true,
    gpuTempC: 65,
    cpuTempC: 60,
    systemTempC: 55,
    gpuName: 'NVIDIA GeForce RTX 4060 Laptop GPU',
    nvidiaTelemetryEnabled: true,
    ocTuningLocked: false,
    nitrosenseMemoryOffsetMhz: 200,
    ...overrides,
  }
}

describe('findBestGpuOverclock', () => {
  it('blocks when the tuner is locked', () => {
    const result = findBestGpuOverclock(base({ ocTuningLocked: true }))
    expect(result.canApply).toBe(false)
    expect(result.blockedReason).toMatch(/Unlock/i)
    expect(result.offsets).toEqual({ coreClockMhz: 0, memoryClockMhz: 0 })
  })

  it('forces stock on battery', () => {
    const result = findBestGpuOverclock(base({ acPluggedIn: false, gpuTempC: 50 }))
    expect(result.tierId).toBe('stock')
    expect(result.canApply).toBe(true)
    expect(result.summary).toMatch(/battery/i)
    expect(result.instructions.some((line) => /AC/i.test(line))).toBe(true)
  })

  it('requires NVIDIA telemetry for live apply', () => {
    const result = findBestGpuOverclock(base({ nvidiaTelemetryEnabled: false }))
    expect(result.canApply).toBe(false)
    expect(result.blockedReason).toMatch(/NVIDIA Telemetry/i)
    expect(result.instructions[0]).toMatch(/Settings/i)
  })

  it('recommends stock when thermals are high', () => {
    const result = findBestGpuOverclock(base({ gpuTempC: 89 }))
    expect(result.tierId).toBe('stock')
    expect(result.summary).toMatch(/too high|Warm package|Stock/i)
  })

  it('recommends mild when warm but not blocked', () => {
    const result = findBestGpuOverclock(base({ gpuTempC: 79, cpuTempC: 76 }))
    expect(result.tierId).toBe('mild')
    expect(result.offsets).toEqual({ coreClockMhz: 50, memoryClockMhz: 100 })
  })

  it('recommends NitroSense Extreme when cool on AC', () => {
    const result = findBestGpuOverclock(base({ gpuTempC: 62 }))
    expect(result.tierId).toBe('nitrosense')
    expect(result.offsets).toEqual({ coreClockMhz: 100, memoryClockMhz: 200 })
    expect(result.instructions.length).toBeGreaterThan(4)
    expect(result.suggestedOcSlotId).toBe('arena')
  })

  it('honors model-specific NitroSense memory offset', () => {
    const result = findBestGpuOverclock(
      base({ gpuTempC: 60, nitrosenseMemoryOffsetMhz: 0 }),
    )
    expect(result.offsets.memoryClockMhz).toBe(0)
    expect(result.offsets.coreClockMhz).toBe(100)
  })
})

describe('stepDownGpuOverclockTier / resultForTier', () => {
  it('steps Extreme → Mild → Stock', () => {
    expect(stepDownGpuOverclockTier('nitrosense')).toBe('mild')
    expect(stepDownGpuOverclockTier('mild')).toBe('stock')
    expect(stepDownGpuOverclockTier('stock')).toBe('stock')
  })

  it('builds a tier result with instructions', () => {
    const result = resultForTier('mild', {}, 'User stepped down after artifacts.')
    expect(result.tierId).toBe('mild')
    expect(result.offsets.coreClockMhz).toBe(50)
    expect(result.summary).toMatch(/stepped down/i)
    expect(result.instructions[0]).toMatch(/Mild/)
  })
})
