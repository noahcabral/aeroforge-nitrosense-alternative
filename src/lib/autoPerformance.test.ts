import { describe, expect, it } from 'vitest'
import {
  autoPerformanceAlreadyApplied,
  decideAutoPerformance,
  hottestTempC,
  peakLoadPercent,
  type AutoPerformanceInput,
} from './autoPerformance'

function baseInput(overrides: Partial<AutoPerformanceInput> = {}): AutoPerformanceInput {
  return {
    acPluggedIn: true,
    cpuUsagePercent: 10,
    gpuUsagePercent: 5,
    cpuTempC: 55,
    gpuTempC: 50,
    systemTempC: 48,
    stickyMode: null,
    ocTuningLocked: false,
    nvidiaTelemetryEnabled: true,
    ...overrides,
  }
}

describe('hottestTempC / peakLoadPercent', () => {
  it('picks the hottest sensor and peak load', () => {
    expect(
      hottestTempC({
        cpuTempC: 70,
        gpuTempC: 82,
        systemTempC: 60,
      }),
    ).toBe(82)
    expect(
      peakLoadPercent({
        cpuUsagePercent: 40,
        gpuUsagePercent: 88,
      }),
    ).toBe(88)
  })
})

describe('decideAutoPerformance', () => {
  it('forces Quiet + stock clocks on battery even under high load', () => {
    const decision = decideAutoPerformance(
      baseInput({
        acPluggedIn: false,
        cpuUsagePercent: 99,
        gpuUsagePercent: 99,
        stickyMode: 'turbo',
      }),
    )
    expect(decision.mode).toBe('quiet')
    expect(decision.powerProfileId).toBe('battery-guard')
    expect(decision.ocSlotId).toBe('silent-uv')
    expect(decision.wantsGpuOffsets).toBe(false)
    expect(decision.reason).toMatch(/battery/i)
  })

  it('forces Quiet on thermal emergency', () => {
    const decision = decideAutoPerformance(
      baseInput({
        cpuTempC: 91,
        gpuUsagePercent: 95,
        stickyMode: 'turbo',
      }),
    )
    expect(decision.mode).toBe('quiet')
    expect(decision.wantsGpuOffsets).toBe(false)
    expect(decision.reason).toMatch(/emergency/i)
  })

  it('backs off to Balanced without OC when warm', () => {
    const decision = decideAutoPerformance(
      baseInput({
        cpuTempC: 86,
        gpuUsagePercent: 95,
        stickyMode: 'turbo',
      }),
    )
    expect(decision.mode).toBe('balanced')
    expect(decision.wantsGpuOffsets).toBe(false)
    expect(decision.reason).toMatch(/backoff/i)
  })

  it('selects Quiet for light AC load', () => {
    const decision = decideAutoPerformance(baseInput({ cpuUsagePercent: 8, gpuUsagePercent: 3 }))
    expect(decision.mode).toBe('quiet')
    expect(decision.powerProfileId).toBe('battery-guard')
    expect(decision.wantsGpuOffsets).toBe(false)
  })

  it('selects Balanced for moderate AC load', () => {
    const decision = decideAutoPerformance(baseInput({ cpuUsagePercent: 40, gpuUsagePercent: 20 }))
    expect(decision.mode).toBe('balanced')
    expect(decision.ocSlotId).toBe('daily')
    expect(decision.wantsGpuOffsets).toBe(false)
  })

  it('selects Performance with NitroSense offsets when cool and telemetry is on', () => {
    const decision = decideAutoPerformance(
      baseInput({
        cpuUsagePercent: 75,
        gpuUsagePercent: 60,
        cpuTempC: 70,
        gpuTempC: 68,
      }),
    )
    expect(decision.mode).toBe('performance')
    expect(decision.powerProfileId).toBe('performance')
    expect(decision.ocSlotId).toBe('creator')
    expect(decision.wantsGpuOffsets).toBe(true)
  })

  it('selects Turbo with NitroSense offsets at very high cool load', () => {
    const decision = decideAutoPerformance(
      baseInput({
        cpuUsagePercent: 95,
        gpuUsagePercent: 90,
        cpuTempC: 72,
        gpuTempC: 70,
      }),
    )
    expect(decision.mode).toBe('turbo')
    expect(decision.ocSlotId).toBe('arena')
    expect(decision.wantsGpuOffsets).toBe(true)
  })

  it('skips GPU offsets when NVIDIA telemetry is disabled', () => {
    const decision = decideAutoPerformance(
      baseInput({
        cpuUsagePercent: 80,
        nvidiaTelemetryEnabled: false,
        cpuTempC: 65,
      }),
    )
    expect(decision.mode).toBe('performance')
    expect(decision.wantsGpuOffsets).toBe(false)
    expect(decision.ocSlotId).toBe('daily')
    expect(decision.reason).toMatch(/telemetry off/i)
  })

  it('skips GPU offsets when the tuner is locked', () => {
    const decision = decideAutoPerformance(
      baseInput({
        cpuUsagePercent: 80,
        ocTuningLocked: true,
        cpuTempC: 65,
      }),
    )
    expect(decision.mode).toBe('performance')
    expect(decision.wantsGpuOffsets).toBe(false)
    expect(decision.reason).toMatch(/locked/i)
  })

  it('skips GPU offsets when warm but below backoff', () => {
    const decision = decideAutoPerformance(
      baseInput({
        cpuUsagePercent: 80,
        cpuTempC: 83,
        gpuTempC: 80,
      }),
    )
    expect(decision.mode).toBe('performance')
    expect(decision.wantsGpuOffsets).toBe(false)
    expect(decision.reason).toMatch(/without GPU offsets/i)
  })

  it('uses hysteresis so Turbo does not immediately drop at 80% load', () => {
    const decision = decideAutoPerformance(
      baseInput({
        stickyMode: 'turbo',
        cpuUsagePercent: 80,
        gpuUsagePercent: 70,
        cpuTempC: 70,
      }),
    )
    expect(decision.mode).toBe('turbo')
    expect(decision.wantsGpuOffsets).toBe(true)
  })

  it('drops from Turbo when load falls below the exit band', () => {
    const decision = decideAutoPerformance(
      baseInput({
        stickyMode: 'turbo',
        cpuUsagePercent: 50,
        gpuUsagePercent: 40,
        cpuTempC: 68,
      }),
    )
    expect(decision.mode).toBe('performance')
  })
})

describe('autoPerformanceAlreadyApplied', () => {
  it('detects matching power + stock clocks', () => {
    expect(
      autoPerformanceAlreadyApplied(
        {
          mode: 'balanced',
          powerProfileId: 'balanced',
          ocSlotId: 'daily',
          wantsGpuOffsets: false,
          reason: 'test',
        },
        {
          powerProfileId: 'balanced',
          ocSlotId: 'daily',
          coreClockMhz: 0,
          memoryClockMhz: 0,
        },
      ),
    ).toBe(true)
  })

  it('requires NitroSense offsets when wantsGpuOffsets is true', () => {
    expect(
      autoPerformanceAlreadyApplied(
        {
          mode: 'performance',
          powerProfileId: 'performance',
          ocSlotId: 'creator',
          wantsGpuOffsets: true,
          reason: 'test',
        },
        {
          powerProfileId: 'performance',
          ocSlotId: 'creator',
          coreClockMhz: 0,
          memoryClockMhz: 0,
        },
      ),
    ).toBe(false)

    expect(
      autoPerformanceAlreadyApplied(
        {
          mode: 'performance',
          powerProfileId: 'performance',
          ocSlotId: 'creator',
          wantsGpuOffsets: true,
          reason: 'test',
        },
        {
          powerProfileId: 'performance',
          ocSlotId: 'creator',
          coreClockMhz: 100,
          memoryClockMhz: 200,
        },
      ),
    ).toBe(true)
  })
})
