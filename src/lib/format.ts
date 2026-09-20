/** Pure formatting helpers for telemetry readouts and status copy. */

export function formatDebugClock(date: Date) {
  return date.toLocaleTimeString([], {
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
    hour12: false,
  })
}

/** `2026-01-01-13-05-09`: the UTC time as a filesystem-safe filename segment. */
export function filenameTimestamp(date = new Date()) {
  return date.toISOString().slice(0, 19).replace(/[:T]/g, '-')
}

export function formatUnixClock(unix: number | null | undefined) {
  if (!unix) {
    return 'Waiting'
  }

  return formatDebugClock(new Date(unix * 1000))
}

export function formatTelemetryValue(value: number | null | undefined, suffix = '') {
  if (value == null) {
    return ''
  }

  return `${value}${suffix}`
}

export function formatFanSpeedMeta(
  rpm: number | null | undefined,
  targetPercent: number | null | undefined,
) {
  const rpmLabel = rpm == null ? null : 'RPM'
  const targetLabel = targetPercent == null ? null : `Target ${targetPercent}%`
  return [rpmLabel, targetLabel].filter(Boolean).join(' | ')
}

export function buildFanSpeedDetail(
  rpm: number | null | undefined,
  targetPercent: number | null | undefined,
  liveDetail: string,
  unavailableDetail = 'Fan telemetry unavailable',
) {
  if (rpm == null && targetPercent == null) {
    return unavailableDetail
  }

  const targetDetail = targetPercent == null ? null : `Target ${targetPercent}%`
  return [targetDetail, rpm == null ? null : liveDetail].filter(Boolean).join(' | ')
}

export function formatWattValue(value: number | null | undefined, maximumFractionDigits = 0) {
  if (value == null || Number.isNaN(value)) {
    return null
  }

  return `${value.toLocaleString(undefined, { maximumFractionDigits })}W`
}

export function formatWattReadout(value: number | null | undefined, maximumFractionDigits = 1) {
  return formatWattValue(value, maximumFractionDigits) ?? '--'
}

export function buildCpuPowerLimitLabel(pl1Label: string | null, pl2Label: string | null) {
  if (pl1Label && pl2Label) {
    return `PL1/PL2 ${pl1Label} / ${pl2Label}`
  }

  if (pl1Label) {
    return `PL1 ${pl1Label}`
  }

  if (pl2Label) {
    return `PL2 ${pl2Label}`
  }

  return null
}

export function buildGpuPowerLimitLabel(
  limitLabel: string | null,
  maxLimit: number | null | undefined,
) {
  if (limitLabel) {
    return `Limit ${limitLabel}`
  }

  const maxLimitLabel = formatWattValue(maxLimit, 1)
  return maxLimitLabel ? `Max ${maxLimitLabel}` : null
}

export function sanitizeIdentityText(value: string | null | undefined) {
  if (!value) {
    return null
  }

  const sanitized = value
    .replace(/\(R\)|\(TM\)/gi, '')
    .replace(/\s+/g, ' ')
    .trim()

  return sanitized.length > 0 ? sanitized : null
}

export function buildHardwareIdentity(
  brand: string | null | undefined,
  model: string | null | undefined,
  fallback: string,
) {
  const cleanBrand = sanitizeIdentityText(brand)
  const cleanModel = sanitizeIdentityText(model)

  if (cleanBrand && cleanModel) {
    const normalizedBrand = cleanBrand.toLowerCase()
    const normalizedModel = cleanModel.toLowerCase()

    if (normalizedModel.includes(normalizedBrand)) {
      return cleanModel
    }

    return `${cleanBrand} ${cleanModel}`
  }

  return cleanModel ?? cleanBrand ?? fallback
}

export function formatLiveBatteryDetail(value: number | null | undefined) {
  if (value == null) {
    return ''
  }

  return `${value}% battery live`
}

export function formatRemainingRuntime(seconds: number | null | undefined) {
  if (seconds == null || !Number.isFinite(seconds) || seconds <= 0) {
    return ''
  }

  const roundedMinutes = Math.max(1, Math.round(seconds / 60))
  const hours = Math.floor(roundedMinutes / 60)
  const minutes = roundedMinutes % 60

  if (hours <= 0) {
    return `${minutes}m`
  }

  if (minutes === 0) {
    return `${hours}h`
  }

  return `${hours}h ${minutes}m`
}

export function formatFrameTime(value: number) {
  if (!Number.isFinite(value) || value <= 0) {
    return ''
  }

  return `${value.toFixed(1)} ms`
}
export function presentPositive(value: number | null | undefined) {
  if (value == null || value <= 0) {
    return null
  }

  return value
}

export function formatPowerSource(acPluggedIn: boolean | null | undefined) {
  if (acPluggedIn == null) {
    return 'Power source unavailable'
  }

  return acPluggedIn ? 'AC Plugged In' : 'Battery Power'
}
