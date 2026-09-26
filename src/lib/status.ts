// Pure text builders for the dashboard, readiness banner and debug panels.
import type {
  ControlLayerStatus,
  FeatureSupport,
  HardwareReadinessSnapshot,
  TelemetrySnapshot,
} from './backend'
import { buildCpuPowerLimitLabel, formatWattValue } from './format'

export type StageFitTarget = 'home' | 'power' | 'fans'

export type StageFitSnapshot = {
  tab: StageFitTarget
  scale: number
  scaledHeight: number
  naturalWidth: number
  naturalHeight: number
  availableWidth: number
  availableHeight: number
  windowWidth: number
  windowHeight: number
  updatedAt: string
}

export function formatStageFitValue(snapshot: StageFitSnapshot | undefined) {
  if (!snapshot) {
    return null
  }

  return `${snapshot.tab} ${snapshot.scale.toFixed(3)} / ${Math.round(snapshot.scaledHeight)}px`
}

export function formatStageFitDetail(snapshot: StageFitSnapshot | undefined) {
  if (!snapshot) {
    return null
  }

  return `natural ${Math.round(snapshot.naturalWidth)}x${Math.round(
    snapshot.naturalHeight,
  )}, available ${Math.round(snapshot.availableWidth)}x${Math.round(
    snapshot.availableHeight,
  )}, window ${snapshot.windowWidth}x${snapshot.windowHeight} at ${snapshot.updatedAt}`
}

export function hasUsableTelemetry(snapshot: TelemetrySnapshot | null | undefined) {
  if (!snapshot) {
    return false
  }

  return Boolean(
    snapshot.cpuTempC > 0 ||
      snapshot.cpuTempAverageC != null ||
      snapshot.gpuTempC > 0 ||
      snapshot.systemTempC > 0 ||
      snapshot.cpuUsagePercent > 0 ||
      snapshot.gpuUsagePercent > 0 ||
      snapshot.gpuMemoryUsagePercent != null ||
      snapshot.gpuPowerLimitW != null ||
      snapshot.gpuPowerDrawW != null ||
      snapshot.cpuPackagePowerW != null ||
      snapshot.cpuPl1W != null ||
      snapshot.cpuPl2W != null ||
      snapshot.cpuClockMhz > 0 ||
      snapshot.gpuClockMhz > 0 ||
      snapshot.cpuFanRpm > 0 ||
      snapshot.gpuFanRpm > 0 ||
      snapshot.batteryPercent > 0 ||
      snapshot.cpuName ||
      snapshot.gpuName ||
      snapshot.systemModel,
  )
}

export function describeTelemetrySource(
  serviceConnected: boolean,
  telemetry: TelemetrySnapshot | null | undefined,
) {
  if (serviceConnected) {
    return 'Service pipe'
  }

  return hasUsableTelemetry(telemetry) ? 'Cached service state' : 'No telemetry'
}

export function describeFeatureSupport(
  feature: FeatureSupport | null | undefined,
  unsupportedDetail: string,
  stagedDetail?: string,
) {
  if (!feature) {
    return null
  }

  if (!feature.available) {
    return unsupportedDetail
  }

  if (!feature.writable) {
    return stagedDetail ?? unsupportedDetail
  }

  return null
}

export function buildHardwareReadinessTitle(snapshot: HardwareReadinessSnapshot) {
  if (snapshot.ready) {
    return 'Hardware controls ready'
  }
  if (!snapshot.service.installed) {
    return 'AeroForge service is not installed'
  }
  if (!snapshot.service.running) {
    return 'AeroForge service is not running'
  }
  if (snapshot.wmi.disabled) {
    return 'Windows Management Instrumentation is disabled'
  }
  if (!snapshot.wmi.service.running) {
    return 'Windows Management Instrumentation is stopped'
  }
  if (!snapshot.acer.acerGamingWmiPresent) {
    return 'Acer firmware control path is unavailable'
  }

  return 'Hardware controls need attention'
}

export function formatLayerState(state: ControlLayerStatus['state']) {
  switch (state) {
    case 'applied':
      return 'Applied'
    case 'failed':
      return 'Failed'
    case 'skipped':
      return 'Skipped'
    case 'unavailable':
      return 'Unavailable'
    case 'unknown':
      return 'Unknown'
  }
}

export function summarizeLayerAttention(statuses: ControlLayerStatus[] | null | undefined) {
  const attention = (statuses ?? []).filter((status) =>
    ['failed', 'unavailable', 'unknown'].includes(status.state),
  )
  if (attention.length === 0) {
    return null
  }

  return attention.map((status) => `${status.label}: ${formatLayerState(status.state)}`).join(' | ')
}

export function buildCpuThermalSummary(
  lowestCoreTemp: number | null | undefined,
  highestCoreTemp: number | null | undefined,
  cpuUsage: number | null | undefined,
) {
  const thermalRange =
    lowestCoreTemp != null && highestCoreTemp != null
      ? `Low ${lowestCoreTemp}C / High ${highestCoreTemp}C`
      : null
  const usageLabel = cpuUsage != null ? `Usage ${cpuUsage}%` : null

  return [thermalRange, usageLabel].filter(Boolean).join(' • ')
}

export function buildPowerDashboardSummary(
  temperatureC: number | null | undefined,
  usagePercent: number | null | undefined,
) {
  const temperatureLabel = temperatureC != null ? `Temp ${temperatureC}C` : null
  const usageLabel = usagePercent != null ? `Usage ${usagePercent}%` : null

  return [temperatureLabel, usageLabel].filter(Boolean).join(' | ')
}

export function buildCpuPowerDashboardSummary(
  temperatureC: number | null | undefined,
  usagePercent: number | null | undefined,
  packagePowerW: number | null | undefined,
  pl1W: number | null | undefined,
  pl2W: number | null | undefined,
) {
  const baseSummary = buildPowerDashboardSummary(temperatureC, usagePercent)
  const powerLabel = packagePowerW != null ? `Power ${formatWattValue(packagePowerW, 1)}` : null
  const limitLabel = buildCpuPowerLimitLabel(
    formatWattValue(pl1W, 1),
    formatWattValue(pl2W, 1),
  )

  return [baseSummary, powerLabel, limitLabel].filter(Boolean).join(' | ')
}

export function buildGpuPowerDashboardSummary(
  temperatureC: number | null | undefined,
  usagePercent: number | null | undefined,
  powerDrawW: number | null | undefined,
  powerLimitW: number | null | undefined,
) {
  const baseSummary = buildPowerDashboardSummary(temperatureC, usagePercent)
  const powerLabel = powerDrawW != null ? `Power ${formatWattValue(powerDrawW, 1)}` : null
  const limitLabel = powerLimitW != null ? `Limit ${formatWattValue(powerLimitW, 1)}` : null

  return [baseSummary, powerLabel, limitLabel].filter(Boolean).join(' | ')
}
