/** Pure fan-curve helpers and Custom-mode templates for the control UI. */

export type CurveTarget = 'cpu' | 'gpu'

export type CurvePoint = {
  temp: number
  speed: number
}

export type CurveSet = Record<CurveTarget, CurvePoint[]>

export const chartWidth = 480
export const chartHeight = 260
export const chartPadding = 24
export const tempMin = 30
export const tempMax = 90
export const speedMin = 0
export const speedMax = 100

export const clamp = (value: number, min: number, max: number) =>
  Math.min(max, Math.max(min, value))

export function pointToChart(point: CurvePoint) {
  const x =
    chartPadding +
    ((point.temp - tempMin) / (tempMax - tempMin)) * (chartWidth - chartPadding * 2)
  const y =
    chartHeight -
    chartPadding -
    ((point.speed - speedMin) / (speedMax - speedMin)) * (chartHeight - chartPadding * 2)

  return { x, y }
}

export function buildCurvePath(points: CurvePoint[]) {
  return points
    .map((point, index) => {
      const { x, y } = pointToChart(point)
      return `${index === 0 ? 'M' : 'L'} ${x} ${y}`
    })
    .join(' ')
}

export function normalizeCurvePoints(points: CurvePoint[]): CurvePoint[] {
  let lastTemp = tempMin - 2
  let lastSpeed = speedMin
  const sorted = points
    .map((point) => ({
      temp: Math.round(clamp(point.temp, tempMin, tempMax)),
      speed: Math.round(clamp(point.speed, speedMin, speedMax)),
    }))
    .sort((left, right) => left.temp - right.temp)

  return sorted.map((point, index) => {
    const remainingPoints = sorted.length - index - 1
    const minTemp = index === 0 ? tempMin : lastTemp + 2
    const maxTemp = tempMax - remainingPoints * 2
    const normalized = {
      temp: clamp(point.temp, minTemp, maxTemp),
      speed: clamp(point.speed, lastSpeed, speedMax),
    }
    lastTemp = normalized.temp
    lastSpeed = normalized.speed
    return normalized
  })
}

export function duplicateCurveSet(curves: CurveSet): CurveSet {
  return {
    cpu: normalizeCurvePoints(curves.cpu),
    gpu: normalizeCurvePoints(curves.gpu),
  }
}

export function interpolateCurveTarget(
  points: CurvePoint[],
  temp: number | null | undefined,
): number | null {
  if (temp == null || points.length === 0) {
    return null
  }

  const roundedTemp = Math.round(temp)
  if (roundedTemp <= points[0].temp) {
    return points[0].speed
  }

  for (let index = 1; index < points.length; index += 1) {
    const lower = points[index - 1]
    const upper = points[index]
    if (roundedTemp > upper.temp) {
      continue
    }

    const tempSpan = Math.max(1, upper.temp - lower.temp)
    const tempOffset = roundedTemp - lower.temp
    const speed = lower.speed + ((upper.speed - lower.speed) * tempOffset) / tempSpan
    return Math.round(clamp(speed, speedMin, speedMax))
  }

  return points[points.length - 1].speed
}

export function otherCurveTarget(target: CurveTarget): CurveTarget {
  return target === 'cpu' ? 'gpu' : 'cpu'
}

export function mirrorCurveSetFromTarget(curves: CurveSet, source: CurveTarget): CurveSet {
  const normalized = duplicateCurveSet(curves)
  return {
    ...normalized,
    [otherCurveTarget(source)]: normalized[source].map((point) => ({ ...point })),
  }
}

export type FanCurveTemplateId = 'silent' | 'balanced' | 'aggressive' | 'cool-match'

export type FanCurveTemplate = {
  id: FanCurveTemplateId
  name: string
  strap: string
  summary: string
  curves: CurveSet
}

/** Built-in Custom curve starting points (software curves, not firmware profile IDs). */
export const FAN_CURVE_TEMPLATES: FanCurveTemplate[] = [
  {
    id: 'silent',
    name: 'Silent',
    strap: 'Low noise first',
    summary: 'Near-floor fans until mid temps, then a safety ramp.',
    curves: {
      cpu: [
        { temp: 30, speed: 0 },
        { temp: 50, speed: 0 },
        { temp: 62, speed: 18 },
        { temp: 74, speed: 48 },
        { temp: 85, speed: 88 },
      ],
      gpu: [
        { temp: 30, speed: 0 },
        { temp: 50, speed: 0 },
        { temp: 62, speed: 16 },
        { temp: 74, speed: 46 },
        { temp: 85, speed: 86 },
      ],
    },
  },
  {
    id: 'balanced',
    name: 'Balanced',
    strap: 'Daily driver',
    summary: 'Progressive ramp for mixed work and light games.',
    curves: {
      cpu: [
        { temp: 30, speed: 8 },
        { temp: 48, speed: 18 },
        { temp: 62, speed: 38 },
        { temp: 74, speed: 68 },
        { temp: 84, speed: 100 },
      ],
      gpu: [
        { temp: 30, speed: 6 },
        { temp: 48, speed: 16 },
        { temp: 62, speed: 36 },
        { temp: 74, speed: 66 },
        { temp: 84, speed: 100 },
      ],
    },
  },
  {
    id: 'aggressive',
    name: 'Aggressive',
    strap: 'Cool first',
    summary: 'Higher baseline speeds for sustained loads.',
    curves: {
      cpu: [
        { temp: 30, speed: 28 },
        { temp: 48, speed: 42 },
        { temp: 60, speed: 62 },
        { temp: 72, speed: 84 },
        { temp: 82, speed: 100 },
      ],
      gpu: [
        { temp: 30, speed: 26 },
        { temp: 48, speed: 40 },
        { temp: 60, speed: 60 },
        { temp: 72, speed: 82 },
        { temp: 82, speed: 100 },
      ],
    },
  },
  {
    id: 'cool-match',
    name: 'Cool Match',
    strap: 'Matches Cool mode',
    summary: 'Editable copy of AeroForge Cool adaptive targets.',
    curves: {
      cpu: [
        { temp: 30, speed: 20 },
        { temp: 50, speed: 35 },
        { temp: 65, speed: 55 },
        { temp: 75, speed: 78 },
        { temp: 82, speed: 100 },
      ],
      gpu: [
        { temp: 30, speed: 20 },
        { temp: 50, speed: 35 },
        { temp: 65, speed: 55 },
        { temp: 75, speed: 78 },
        { temp: 82, speed: 100 },
      ],
    },
  },
]
