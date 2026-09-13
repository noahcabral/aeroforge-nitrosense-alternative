import type {
  CapabilitySnapshot,
  HardwareReadinessSnapshot,
  LiveControlSnapshot,
  ServiceStatus,
  TelemetrySnapshot,
} from './backend'

export type DiagnosticsFrameStats = {
  averageMs: number
  maxMs: number
  fps: number
  longFrameCount: number
  updatedAt: string
}

export type DiagnosticsBundle = {
  generatedAt: string
  appVersion: string
  shellStatus: string
  statusMessage: string
  serviceConnected: boolean
  serviceStatus: ServiceStatus | null
  hardwareReadiness: HardwareReadinessSnapshot | null
  capabilities: CapabilitySnapshot | null
  activePowerProfile: string
  activeFanProfile: string
  processorState: { min: number; max: number }
  customPowerBase: string
  processorStateControlEnabled: boolean
  smartChargingEnabled: boolean
  nvidiaTelemetryEnabled: boolean
  blueLightFilterEnabled: boolean
  telemetry: TelemetrySnapshot | null
  liveControls: LiveControlSnapshot | null
  lastBackendError: string | null
  lastBackendPollAt: string
  telemetrySourceLabel: string
  recentEvents: string[]
  frameStats: DiagnosticsFrameStats
  performanceLogPath: string | null
}

export function buildDiagnosticsBundle(input: DiagnosticsBundle): DiagnosticsBundle {
  return {
    ...input,
    recentEvents: input.recentEvents.slice(-80),
  }
}

export function formatDiagnosticsText(bundle: DiagnosticsBundle): string {
  const lines: string[] = []
  const push = (line = '') => {
    lines.push(line)
  }

  push('AeroForge Control — Diagnostics Snapshot')
  push(`Generated: ${bundle.generatedAt}`)
  push(`App version: ${bundle.appVersion}`)
  push(`Shell: ${bundle.shellStatus}`)
  push(`Status: ${bundle.statusMessage}`)
  push(`Service connected: ${bundle.serviceConnected ? 'yes' : 'no'}`)
  push(`Telemetry source: ${bundle.telemetrySourceLabel}`)
  push(`Last poll: ${bundle.lastBackendPollAt}`)
  if (bundle.lastBackendError) {
    push(`Last backend error: ${bundle.lastBackendError}`)
  }
  push()

  push('--- Active modes ---')
  push(`Power: ${bundle.activePowerProfile}`)
  push(`Fans: ${bundle.activeFanProfile}`)
  push(
    `Processor policy: ${bundle.processorStateControlEnabled ? 'on' : 'off'} (${bundle.processorState.min}-${bundle.processorState.max}%) base=${bundle.customPowerBase}`,
  )
  push(
    `Smart charge: ${bundle.smartChargingEnabled ? 'on' : 'off'} | NVIDIA telemetry: ${bundle.nvidiaTelemetryEnabled ? 'on' : 'off'} | Blue light: ${bundle.blueLightFilterEnabled ? 'on' : 'off'}`,
  )
  push()

  if (bundle.serviceStatus) {
    const service = bundle.serviceStatus
    push('--- Service ---')
    push(`Name: ${service.serviceName}`)
    push(`Pipe: ${service.pipeName}`)
    push(`Version: ${service.version ?? 'unknown'}`)
    push(`Workers: ${service.workerCount}`)
    push(`Detail: ${service.detail}`)
    for (const worker of service.workers) {
      push(
        `  - ${worker.name}: ${worker.state} (interval ${worker.intervalSeconds}s)${
          worker.lastError ? ` error=${worker.lastError}` : ''
        }`,
      )
    }
    push()
  }

  if (bundle.hardwareReadiness) {
    const ready = bundle.hardwareReadiness
    push('--- Hardware readiness ---')
    push(`Ready: ${ready.ready ? 'yes' : 'no'}`)
    push(`Detail: ${ready.detail}`)
    push(
      `Service: installed=${ready.service.installed} running=${ready.service.running} startType=${ready.service.startType ?? 'n/a'}`,
    )
    push(
      `WMI: running=${ready.wmi.service.running} disabled=${ready.wmi.disabled} needsRepair=${ready.wmi.needsRepair}`,
    )
    push(
      `Acer: vendor=${ready.acer.systemVendor ?? '?'} model=${ready.acer.systemModel ?? '?'} wmi=${ready.acer.acerGamingWmiPresent} anv15_51=${ready.acer.anv15_51}`,
    )
    push()
  }

  if (bundle.capabilities) {
    const caps = bundle.capabilities
    push('--- Capabilities ---')
    for (const [name, feature] of Object.entries({
      powerProfiles: caps.powerProfiles,
      fanProfiles: caps.fanProfiles,
      fanCurves: caps.fanCurves,
      smartCharging: caps.smartCharging,
      usbPower: caps.usbPower,
      blueLightFilter: caps.blueLightFilter,
      gpuTuning: caps.gpuTuning,
      bootLogo: caps.bootLogo,
    })) {
      push(
        `${name}: available=${feature.available} writable=${feature.writable} elev=${feature.requiresElevation}`,
      )
    }
    if (caps.notes.length > 0) {
      push(`Notes: ${caps.notes.join(' | ')}`)
    }
    push()
  }

  if (bundle.telemetry) {
    const t = bundle.telemetry
    push('--- Telemetry ---')
    push(`Identity: ${t.systemVendor ?? '?'} ${t.systemModel ?? '?'}`)
    push(`CPU: ${t.cpuName ?? t.cpuBrand ?? '?'} @ ${t.cpuClockMhz} MHz, ${t.cpuUsagePercent}%`)
    push(
      `CPU temps: avg=${t.cpuTempAverageC ?? t.cpuTempC}C low=${t.cpuTempLowestCoreC ?? '?'} high=${t.cpuTempHighestCoreC ?? '?'}`,
    )
    push(
      `CPU power: package=${t.cpuPackagePowerW ?? '?'}W PL1=${t.cpuPl1W ?? '?'}W PL2=${t.cpuPl2W ?? '?'}W`,
    )
    push(`GPU: ${t.gpuName ?? t.gpuBrand ?? '?'} @ ${t.gpuClockMhz} MHz, ${t.gpuUsagePercent}%`)
    push(
      `GPU temp=${t.gpuTempC}C power=${t.gpuPowerDrawW ?? '?'}W limit=${t.gpuPowerLimitW ?? '?'}W`,
    )
    push(`Fans: CPU ${t.cpuFanRpm} RPM / GPU ${t.gpuFanRpm} RPM`)
    push(
      `Battery: ${t.batteryPercent}% AC=${t.acPluggedIn ? 'yes' : 'no'} remainingSec=${t.batteryLifeRemainingSec ?? 'n/a'}`,
    )
    push()
  }

  if (bundle.liveControls) {
    const live = bundle.liveControls
    push('--- Live control (service) ---')
    push(`Power detail: ${live.lastApplyDetail || '(none)'}`)
    if (live.lastError) {
      push(`Power error: ${live.lastError}`)
    }
    for (const layer of live.lastPowerLayerStatuses) {
      push(`  power/${layer.id}: ${layer.state} — ${layer.detail}`)
    }
    push(`Fan detail: ${live.lastFanApplyDetail || '(none)'}`)
    if (live.lastFanError) {
      push(`Fan error: ${live.lastFanError}`)
    }
    for (const layer of live.lastFanLayerStatuses) {
      push(`  fan/${layer.id}: ${layer.state} — ${layer.detail}`)
    }
    push()
  }

  push('--- UI performance ---')
  push(
    `Frame avg ${bundle.frameStats.averageMs.toFixed(1)}ms max ${bundle.frameStats.maxMs.toFixed(1)}ms fps ${Math.round(bundle.frameStats.fps)} long ${bundle.frameStats.longFrameCount} @ ${bundle.frameStats.updatedAt}`,
  )
  if (bundle.performanceLogPath) {
    push(`Perf log: ${bundle.performanceLogPath}`)
  }
  push()

  if (bundle.recentEvents.length > 0) {
    push('--- Recent debug events ---')
    for (const event of bundle.recentEvents.slice(-40)) {
      push(`* ${event}`)
    }
    push()
  }

  push('--- End ---')
  return lines.join('\n')
}
