import { describe, expect, it } from 'vitest'
import {
  FAN_CURVE_TEMPLATES,
  buildCurvePath,
  clamp,
  duplicateCurveSet,
  interpolateCurveTarget,
  mirrorCurveSetFromTarget,
  normalizeCurvePoints,
  otherCurveTarget,
  pointToChart,
  speedMax,
  speedMin,
  tempMax,
  tempMin,
  type CurvePoint,
} from './curves'

/** Invariants the service relies on: sorted, ≥2C apart, non-decreasing speed, in range. */
function expectWellFormed(points: CurvePoint[]) {
  for (let index = 0; index < points.length; index += 1) {
    const point = points[index]
    expect(Number.isInteger(point.temp)).toBe(true)
    expect(Number.isInteger(point.speed)).toBe(true)
    expect(point.temp).toBeGreaterThanOrEqual(tempMin)
    expect(point.temp).toBeLessThanOrEqual(tempMax)
    expect(point.speed).toBeGreaterThanOrEqual(speedMin)
    expect(point.speed).toBeLessThanOrEqual(speedMax)
    if (index > 0) {
      expect(point.temp - points[index - 1].temp).toBeGreaterThanOrEqual(2)
      expect(point.speed).toBeGreaterThanOrEqual(points[index - 1].speed)
    }
  }
}

describe('clamp', () => {
  it('bounds values on both sides', () => {
    expect(clamp(5, 0, 10)).toBe(5)
    expect(clamp(-1, 0, 10)).toBe(0)
    expect(clamp(11, 0, 10)).toBe(10)
  })
})

describe('normalizeCurvePoints', () => {
  it('leaves an already-valid curve untouched', () => {
    const points = [
      { temp: 30, speed: 10 },
      { temp: 50, speed: 30 },
      { temp: 70, speed: 60 },
      { temp: 90, speed: 100 },
    ]
    expect(normalizeCurvePoints(points)).toEqual(points)
  })

  it('sorts by temperature', () => {
    const result = normalizeCurvePoints([
      { temp: 70, speed: 60 },
      { temp: 30, speed: 10 },
      { temp: 50, speed: 30 },
    ])
    expect(result.map((point) => point.temp)).toEqual([30, 50, 70])
    expectWellFormed(result)
  })

  it('enforces at least 2C spacing between neighbours', () => {
    const result = normalizeCurvePoints([
      { temp: 40, speed: 10 },
      { temp: 40, speed: 20 },
      { temp: 41, speed: 30 },
    ])
    expect(result.map((point) => point.temp)).toEqual([40, 42, 44])
    expectWellFormed(result)
  })

  it('never lets speed decrease as temperature rises', () => {
    const result = normalizeCurvePoints([
      { temp: 30, speed: 50 },
      { temp: 50, speed: 20 },
      { temp: 70, speed: 80 },
    ])
    expect(result.map((point) => point.speed)).toEqual([50, 50, 80])
    expectWellFormed(result)
  })

  it('clamps and rounds out-of-range or fractional input', () => {
    const result = normalizeCurvePoints([
      { temp: -10, speed: -5 },
      { temp: 55.4, speed: 33.6 },
      { temp: 200, speed: 150 },
    ])
    expect(result).toEqual([
      { temp: 30, speed: 0 },
      { temp: 55, speed: 34 },
      { temp: 90, speed: 100 },
    ])
    expectWellFormed(result)
  })

  it('reserves headroom so trailing points can still fit under tempMax', () => {
    const result = normalizeCurvePoints([
      { temp: 90, speed: 10 },
      { temp: 90, speed: 20 },
      { temp: 90, speed: 30 },
    ])
    expect(result.map((point) => point.temp)).toEqual([86, 88, 90])
    expectWellFormed(result)
  })

  it('handles an empty curve', () => {
    expect(normalizeCurvePoints([])).toEqual([])
  })
})

describe('duplicateCurveSet', () => {
  it('returns a deep copy that does not alias the input', () => {
    const source = {
      cpu: [{ temp: 30, speed: 0 }],
      gpu: [{ temp: 30, speed: 0 }],
    }
    const copy = duplicateCurveSet(source)
    expect(copy).toEqual(source)
    expect(copy.cpu).not.toBe(source.cpu)
    expect(copy.cpu[0]).not.toBe(source.cpu[0])
  })
})

describe('mirrorCurveSetFromTarget', () => {
  it('copies the source zone onto the other zone', () => {
    const curves = {
      cpu: [
        { temp: 30, speed: 10 },
        { temp: 60, speed: 50 },
      ],
      gpu: [
        { temp: 30, speed: 0 },
        { temp: 60, speed: 0 },
      ],
    }
    const mirrored = mirrorCurveSetFromTarget(curves, 'cpu')
    expect(mirrored.gpu).toEqual(mirrored.cpu)
    expect(mirrored.gpu).not.toBe(mirrored.cpu)
    expect(mirrored.gpu[0]).not.toBe(mirrored.cpu[0])
  })

  it('mirrors in either direction', () => {
    const curves = {
      cpu: [{ temp: 30, speed: 0 }],
      gpu: [{ temp: 40, speed: 40 }],
    }
    expect(mirrorCurveSetFromTarget(curves, 'gpu').cpu).toEqual([{ temp: 40, speed: 40 }])
  })
})

describe('otherCurveTarget', () => {
  it('flips between cpu and gpu', () => {
    expect(otherCurveTarget('cpu')).toBe('gpu')
    expect(otherCurveTarget('gpu')).toBe('cpu')
  })
})

describe('interpolateCurveTarget', () => {
  const points = [
    { temp: 30, speed: 0 },
    { temp: 50, speed: 20 },
    { temp: 70, speed: 80 },
  ]

  it('returns null for missing temperature or empty curve', () => {
    expect(interpolateCurveTarget(points, null)).toBeNull()
    expect(interpolateCurveTarget(points, undefined)).toBeNull()
    expect(interpolateCurveTarget([], 50)).toBeNull()
  })

  it('clamps to the first and last point outside the curve', () => {
    expect(interpolateCurveTarget(points, 10)).toBe(0)
    expect(interpolateCurveTarget(points, 30)).toBe(0)
    expect(interpolateCurveTarget(points, 95)).toBe(80)
  })

  it('returns exact values at the knots', () => {
    expect(interpolateCurveTarget(points, 50)).toBe(20)
    expect(interpolateCurveTarget(points, 70)).toBe(80)
  })

  it('interpolates linearly between knots and rounds', () => {
    expect(interpolateCurveTarget(points, 40)).toBe(10)
    expect(interpolateCurveTarget(points, 60)).toBe(50)
    expect(interpolateCurveTarget(points, 55)).toBe(35)
    expect(interpolateCurveTarget(points, 55.4)).toBe(35)
  })

  it('does not divide by zero when two knots share a temperature', () => {
    expect(
      interpolateCurveTarget(
        [
          { temp: 50, speed: 10 },
          { temp: 50, speed: 90 },
        ],
        50,
      ),
    ).toBe(10)
  })
})

describe('chart geometry', () => {
  it('maps the temperature/speed extremes to the padded chart corners', () => {
    const bottomLeft = pointToChart({ temp: tempMin, speed: speedMin })
    const topRight = pointToChart({ temp: tempMax, speed: speedMax })
    expect(bottomLeft).toEqual({ x: 24, y: 236 })
    expect(topRight).toEqual({ x: 456, y: 24 })
  })

  it('builds an SVG path starting with M and continuing with L', () => {
    expect(
      buildCurvePath([
        { temp: tempMin, speed: speedMin },
        { temp: tempMax, speed: speedMax },
      ]),
    ).toBe('M 24 236 L 456 24')
    expect(buildCurvePath([])).toBe('')
  })
})

describe('FAN_CURVE_TEMPLATES', () => {
  it('ship well-formed curves for both zones', () => {
    for (const template of FAN_CURVE_TEMPLATES) {
      expectWellFormed(template.curves.cpu)
      expectWellFormed(template.curves.gpu)
      expect(normalizeCurvePoints(template.curves.cpu)).toEqual(template.curves.cpu)
      expect(normalizeCurvePoints(template.curves.gpu)).toEqual(template.curves.gpu)
    }
  })

  it('have unique ids', () => {
    const ids = FAN_CURVE_TEMPLATES.map((template) => template.id)
    expect(new Set(ids).size).toBe(ids.length)
  })
})
