import { describe, expect, it } from 'vitest'
import {
  buildCpuPowerLimitLabel,
  buildFanSpeedDetail,
  buildGpuPowerLimitLabel,
  buildHardwareIdentity,
  formatFanSpeedMeta,
  formatFrameTime,
  formatLiveBatteryDetail,
  formatPowerSource,
  formatRemainingRuntime,
  formatTelemetryValue,
  formatUnixClock,
  formatWattReadout,
  formatWattValue,
  presentPositive,
  sanitizeIdentityText,
} from './format'

describe('formatTelemetryValue', () => {
  it('renders the value with an optional suffix and blanks null', () => {
    expect(formatTelemetryValue(42)).toBe('42')
    expect(formatTelemetryValue(42, 'C')).toBe('42C')
    expect(formatTelemetryValue(null)).toBe('')
    expect(formatTelemetryValue(undefined, '%')).toBe('')
  })
})

describe('fan speed copy', () => {
  it('joins the parts that are present', () => {
    expect(formatFanSpeedMeta(3200, 55)).toBe('RPM | Target 55%')
    expect(formatFanSpeedMeta(null, 55)).toBe('Target 55%')
    expect(formatFanSpeedMeta(3200, null)).toBe('RPM')
    expect(formatFanSpeedMeta(null, null)).toBe('')
  })

  it('falls back to the unavailable copy only when nothing is known', () => {
    expect(buildFanSpeedDetail(null, null, 'Live')).toBe('Fan telemetry unavailable')
    expect(buildFanSpeedDetail(null, null, 'Live', 'Custom')).toBe('Custom')
    expect(buildFanSpeedDetail(3200, 55, 'Live')).toBe('Target 55% | Live')
    expect(buildFanSpeedDetail(null, 55, 'Live')).toBe('Target 55%')
    expect(buildFanSpeedDetail(3200, null, 'Live')).toBe('Live')
  })
})

describe('watt formatting', () => {
  it('formats watts with the requested precision', () => {
    expect(formatWattValue(45.678)).toBe('46W')
    expect(formatWattValue(45.678, 1)).toBe('45.7W')
    expect(formatWattValue(null)).toBeNull()
    expect(formatWattValue(Number.NaN)).toBeNull()
  })

  it('readout uses a dash placeholder', () => {
    expect(formatWattReadout(12.34)).toBe('12.3W')
    expect(formatWattReadout(null)).toBe('--')
  })

  it('builds PL1/PL2 labels from whichever limits are known', () => {
    expect(buildCpuPowerLimitLabel('45W', '95W')).toBe('PL1/PL2 45W / 95W')
    expect(buildCpuPowerLimitLabel('45W', null)).toBe('PL1 45W')
    expect(buildCpuPowerLimitLabel(null, '95W')).toBe('PL2 95W')
    expect(buildCpuPowerLimitLabel(null, null)).toBeNull()
  })

  it('prefers the live GPU limit over the max limit', () => {
    expect(buildGpuPowerLimitLabel('115W', 140)).toBe('Limit 115W')
    expect(buildGpuPowerLimitLabel(null, 140)).toBe('Max 140W')
    expect(buildGpuPowerLimitLabel(null, null)).toBeNull()
  })
})

describe('hardware identity', () => {
  it('strips trademark noise and collapses whitespace', () => {
    expect(sanitizeIdentityText('Intel(R) Core(TM)   i7')).toBe('Intel Core i7')
    expect(sanitizeIdentityText('   ')).toBeNull()
    expect(sanitizeIdentityText(null)).toBeNull()
  })

  it('avoids repeating the brand when the model already contains it', () => {
    expect(buildHardwareIdentity('NVIDIA', 'NVIDIA GeForce RTX 4060', 'GPU')).toBe(
      'NVIDIA GeForce RTX 4060',
    )
    expect(buildHardwareIdentity('Acer', 'Nitro ANV15-51', 'Laptop')).toBe('Acer Nitro ANV15-51')
    expect(buildHardwareIdentity(null, 'Nitro', 'Laptop')).toBe('Nitro')
    expect(buildHardwareIdentity('Acer', null, 'Laptop')).toBe('Acer')
    expect(buildHardwareIdentity(null, null, 'Laptop')).toBe('Laptop')
  })
})

describe('battery and runtime copy', () => {
  it('formats the live battery detail', () => {
    expect(formatLiveBatteryDetail(73)).toBe('73% battery live')
    expect(formatLiveBatteryDetail(null)).toBe('')
  })

  it('formats remaining runtime in hours and minutes', () => {
    expect(formatRemainingRuntime(45)).toBe('1m')
    expect(formatRemainingRuntime(30 * 60)).toBe('30m')
    expect(formatRemainingRuntime(60 * 60)).toBe('1h')
    expect(formatRemainingRuntime(95 * 60)).toBe('1h 35m')
    expect(formatRemainingRuntime(0)).toBe('')
    expect(formatRemainingRuntime(Number.POSITIVE_INFINITY)).toBe('')
    expect(formatRemainingRuntime(null)).toBe('')
  })

  it('describes the power source', () => {
    expect(formatPowerSource(true)).toBe('AC Plugged In')
    expect(formatPowerSource(false)).toBe('Battery Power')
    expect(formatPowerSource(null)).toBe('Power source unavailable')
  })
})

describe('misc', () => {
  it('formats frame times and blanks non-positive values', () => {
    expect(formatFrameTime(16.666)).toBe('16.7 ms')
    expect(formatFrameTime(0)).toBe('')
    expect(formatFrameTime(Number.NaN)).toBe('')
  })

  it('presentPositive filters zero and null', () => {
    expect(presentPositive(5)).toBe(5)
    expect(presentPositive(0)).toBeNull()
    expect(presentPositive(-1)).toBeNull()
    expect(presentPositive(null)).toBeNull()
  })

  it('formatUnixClock reports Waiting for a missing timestamp', () => {
    expect(formatUnixClock(null)).toBe('Waiting')
    expect(formatUnixClock(0)).toBe('Waiting')
    expect(formatUnixClock(1_700_000_000)).toMatch(/^\d{2}:\d{2}:\d{2}$/)
  })
})
