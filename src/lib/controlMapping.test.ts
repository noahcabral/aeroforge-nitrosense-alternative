import { describe, expect, it } from 'vitest'
import {
  buildCustomOcStrap,
  builtInOcProfileSlots,
  defaultCustomOcSlot,
  fromBackendCurveSet,
  fromBackendGpuTuningState,
  fromBackendOcPreset,
  toBackendCurveSet,
  toBackendGpuTuningState,
  toBackendOcPreset,
} from './controlMapping'
import { FAN_CURVE_TEMPLATES } from './curves'

describe('control mapping', () => {
  it('round-trips GPU tuning and OC slots through the backend shape', () => {
    const tuning = { coreClock: 120, memoryClock: -40, voltageOffset: -25, powerLimit: 95, tempLimit: 80 }
    expect(fromBackendGpuTuningState(toBackendGpuTuningState(tuning))).toEqual(tuning)

    for (const slot of [...builtInOcProfileSlots, defaultCustomOcSlot]) {
      expect(fromBackendOcPreset(toBackendOcPreset(slot))).toEqual({ ...slot, isCustom: Boolean(slot.isCustom) })
    }
  })

  it('round-trips fan curves with backend field names', () => {
    const curves = FAN_CURVE_TEMPLATES[0].curves
    const backend = toBackendCurveSet(curves)
    expect(backend.cpu[0]).toEqual({ tempC: curves.cpu[0].temp, speedPercent: curves.cpu[0].speed })
    expect(fromBackendCurveSet(backend)).toEqual(curves)
  })

  it('signs every offset in the custom OC strap', () => {
    expect(buildCustomOcStrap({ coreClock: 50, memoryClock: -100, voltageOffset: 0, powerLimit: 100, tempLimit: 83 })).toBe(
      'Core +50 / Mem -100 / +0mV',
    )
  })
})
