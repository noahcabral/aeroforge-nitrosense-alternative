import {
  type ChangeEvent,
  useEffect,
  useRef,
  useState,
} from 'react'
import { listen } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'
import './App.css'
import aeroforgeMark from './assets/aeroforge-mark.png'
import { BootSplashPreview } from './components/BootSplashPreview'
import { CurveTemplateBar } from './components/CurveTemplateBar'
import { FanCurvePanel } from './components/FanCurvePanel'
import { HardwareReadinessBanner } from './components/HardwareReadinessBanner'
import { HomeFanReadoutCard } from './components/HomeFanReadoutCard'
import { HomeTemperatureDial } from './components/HomeTemperatureDial'
import { LayerStatusStrip } from './components/LayerStatusStrip'
import { MetricCard } from './components/MetricCard'
import { OverclockSlider } from './components/OverclockSlider'
import { PresetPanel } from './components/PresetPanel'
import { ProcessorStateControl } from './components/ProcessorStateControl'
import { ShortcutsOverlay } from './components/ShortcutsOverlay'
import { ToastStack, type AppToast } from './components/ToastStack'
import {
  applyAutoRefreshRate,
  applyBlueLightFilter,
  applyBootLogo,
  applyCustomFanCurves,
  applyFanProfile,
  applyGpuTuning,
  applyPowerProfile,
  applySmartCharging,
  appendPerformanceLog,
  checkForUpdates,
  getBackendBootstrap,
  getHardwareReadinessSnapshot,
  getBackendPollSnapshot,
  getLiveControlSnapshot,
  installStagedUpdate,
  repairHardwarePrerequisites,
  requestServiceHeal,
  saveControlSnapshot,
  setNvidiaTelemetryEnabled,
  showUpdateNotification,
  stageUpdateDownload,
  type CapabilitySnapshot,
  type ControlLayerStatus,
  type ControlSnapshot,
  type BootArtId,
  type CustomPowerBaseId,
  type FeatureSupport,
  type HardwareReadinessSnapshot,
  type LiveControlSnapshot,
  type PerformanceLogEvent,
  type ServiceStatus,
  type TelemetrySnapshot,
  type UpdateStatus,
} from './lib/backend'
import { bootArtwork, type BootArt } from './lib/bootArt'
import {
  buildJpegBootLogoName,
  prepareBootLogoUpload,
  preparePresetBootLogo,
} from './lib/bootLogo'
import {
  chartHeight,
  chartPadding,
  chartWidth,
  clamp,
  duplicateCurveSet,
  FAN_CURVE_TEMPLATES,
  interpolateCurveTarget,
  mirrorCurveSetFromTarget,
  speedMax,
  speedMin,
  tempMax,
  tempMin,
  type CurveSet,
  type CurveTarget,
  type FanCurveTemplate,
} from './lib/curves'
import { buildDiagnosticsBundle, formatDiagnosticsText } from './lib/diagnostics'
import { copyTextToClipboard, downloadTextFile } from './lib/dom'
import { describeError, withTimeout } from './lib/errors'
import {
  buildCpuPowerLimitLabel,
  buildFanSpeedDetail,
  buildGpuPowerLimitLabel,
  buildHardwareIdentity,
  formatDebugClock,
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
} from './lib/format'
import {
  KEYBOARD_SHORTCUT_ENTRIES,
  KEYBOARD_SHORTCUT_HINT,
  parseControlShortcut,
} from './lib/keyboard'
import {
  buildPresetPack,
  mergePresetPack,
  parsePresetPack,
} from './lib/presetTransfer'
import {
  buildPortableSettings,
  defaultSettingsExportFilename,
  parsePortableSettings,
  serializePortableSettings,
} from './lib/settingsPortable'

type ControlTab = 'home' | 'power' | 'fans' | 'personal' | 'debug'
type PersonalSection = 'updates' | 'charge' | 'screen' | 'boot' | 'backup'
type UpdateChannel = ControlSnapshot['personalSettings']['updateChannel']
type UpdateAction = 'check' | 'stage' | 'install'
type SettingsActionPending =
  | null
  | 'smart-charge'
  | 'blue-light'
  | 'boot-logo'
  | 'refresh-rate'
  | 'nvidia-telemetry'
  | 'hardware-repair'
  | 'hardware-start'
type TrayControlEventPayload = {
  action: string
  detail: string
  controls: ControlSnapshot
}
type TrayControlErrorPayload = {
  action: string
  detail: string
}

type DragState = {
  target: CurveTarget
  index: number
}

type FrameStats = {
  averageMs: number
  maxMs: number
  fps: number
  longFrameCount: number
  sampleWindowMs: number
  updatedAt: string
}

type StageFitTarget = 'home' | 'power' | 'fans'

type StageFitSnapshot = {
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

type PerformanceLogState = {
  path: string | null
  lastFlushAt: string
  pendingCount: number
  eventCount: number
  lastError: string | null
}

type PowerProfile = {
  id: 'battery-guard' | 'balanced' | 'performance' | 'turbo' | 'custom'
  name: string
  strap: string
  summary: string
  wattage: string
  runtime: string
}

type FanProfile = {
  id: ControlSnapshot['activeFanProfile']
  name: string
  strap: string
  summary: string
  badge: string
}

type PersistControlOverrides = {
  activePowerProfile?: PowerProfile['id']
  activeFanProfile?: FanProfile['id']
  customProcessorState?: { min: number; max: number }
  customPowerBase?: CustomPowerBaseId
  customCurves?: CurveSet
  fanPresets?: FanControlPreset[]
  powerPresets?: PowerControlPreset[]
  quickPresets?: QuickControlPreset[]
  fanSyncLockEnabled?: boolean
  smartChargingEnabled?: boolean
  processorStateControlEnabled?: boolean
  nvidiaTelemetryEnabled?: boolean
  autoRefreshRateOnBatteryEnabled?: boolean
  autoRefreshRateRestoreHz?: number | null
  blueLightFilterEnabled?: boolean
  selectedBootArt?: string
  customBootFilename?: string
  updateChannel?: UpdateChannel
  checkForUpdatesOnLaunch?: boolean
}

type FinalizeCustomCurveOptions = {
  activateCustom?: boolean
  fanSyncLockState?: boolean
  statusMessage?: string
}

type GpuTuningState = {
  coreClock: number
  memoryClock: number
  voltageOffset: number
  powerLimit: number
  tempLimit: number
}

type OcProfileSlot = {
  id: string
  label: string
  name: string
  strap: string
  settings: GpuTuningState
  isCustom?: boolean
}

type FanControlPreset = ControlSnapshot['fanPresets'][number]
type PowerControlPreset = ControlSnapshot['powerPresets'][number]
type QuickControlPreset = ControlSnapshot['quickPresets'][number]

const powerProfiles: PowerProfile[] = [
  {
    id: 'battery-guard',
    name: 'Quiet',
    strap: 'Lowest-noise operating mode',
    summary:
      'Enables the direct Whisper quiet path and then layers a conservative processor policy for low-noise sessions.',
    wattage: '28W ceiling',
    runtime: '7h 10m est.',
  },
  {
    id: 'balanced',
    name: 'Balanced',
    strap: 'Daily mixed workload',
    summary: 'Balanced thermals with responsive bursts for editing, browsing, and play.',
    wattage: '45W ceiling',
    runtime: '5h 40m est.',
  },
  {
    id: 'performance',
    name: 'Performance',
    strap: 'Firmware performance preset',
    summary: 'Uses the Acer performance preset for higher sustained package power without forcing the top turbo state.',
    wattage: 'Performance firmware limit',
    runtime: 'AC preferred',
  },
  {
    id: 'turbo',
    name: 'Turbo',
    strap: 'Highest firmware turbo state',
    summary: 'Pins the platform to the confirmed Acer turbo mode for the most aggressive gaming headroom.',
    wattage: 'Turbo firmware limit',
    runtime: 'AC priority',
  },
  {
    id: 'custom',
    name: 'Custom',
    strap: 'Manual processor policy',
    summary: 'Tune minimum and maximum processor state for a personal balance of heat and responsiveness.',
    wattage: 'Variable',
    runtime: 'Adaptive',
  },
]

const customPowerBaseOptions: {
  id: CustomPowerBaseId
  name: string
  summary: string
}[] = [
  { id: 'balanced', name: 'Balanced', summary: 'Starts from Acer balanced firmware behavior.' },
  {
    id: 'performance',
    name: 'Performance',
    summary: 'Starts from Acer performance firmware behavior.',
  },
  { id: 'turbo', name: 'Turbo', summary: 'Starts from Acer turbo firmware behavior.' },
]

const customPowerBaseCeilingLabels: Record<CustomPowerBaseId, string> = {
  balanced: 'Balanced base ceiling',
  performance: 'Performance base ceiling',
  turbo: 'Turbo base ceiling',
}

const fanProfiles: FanProfile[] = [
  {
    id: 'auto',
    name: 'Auto',
    strap: 'Adaptive cooling',
    summary: 'Balances airflow and acoustics automatically for mixed work and gaming.',
    badge: 'A',
  },
  {
    id: 'cool',
    name: 'Cool',
    strap: 'Cooler adaptive curve',
    summary:
      'Tracks temperature like Auto, but asks for stronger airflow before heat builds up.',
    badge: 'CO',
  },
  {
    id: 'max',
    name: 'Max',
    strap: 'Cooling first',
    summary: 'Pins both fans high for the lowest thermals and the most aggressive airflow.',
    badge: 'M',
  },
  {
    id: 'custom',
    name: 'Custom',
    strap: 'Hand tuned',
    summary: 'Direct control over separate GPU and CPU fan curves for both thermal zones.',
    badge: 'C',
  },
]

const presetCurves: Record<FanProfile['id'], CurveSet> = {
  auto: {
    cpu: [
      { temp: 30, speed: 18 },
      { temp: 45, speed: 24 },
      { temp: 58, speed: 38 },
      { temp: 72, speed: 58 },
      { temp: 88, speed: 82 },
    ],
    gpu: [
      { temp: 30, speed: 16 },
      { temp: 45, speed: 22 },
      { temp: 60, speed: 34 },
      { temp: 74, speed: 55 },
      { temp: 87, speed: 78 },
    ],
  },
  cool: {
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
  max: {
    cpu: [
      { temp: 30, speed: 36 },
      { temp: 45, speed: 52 },
      { temp: 58, speed: 69 },
      { temp: 72, speed: 86 },
      { temp: 88, speed: 100 },
    ],
    gpu: [
      { temp: 30, speed: 34 },
      { temp: 45, speed: 49 },
      { temp: 60, speed: 66 },
      { temp: 74, speed: 84 },
      { temp: 87, speed: 100 },
    ],
  },
  custom: {
    cpu: [
      { temp: 30, speed: 2 },
      { temp: 49, speed: 2 },
      { temp: 65, speed: 22 },
      { temp: 74, speed: 64 },
      { temp: 80, speed: 100 },
    ],
    gpu: [
      { temp: 30, speed: 2 },
      { temp: 49, speed: 2 },
      { temp: 65, speed: 22 },
      { temp: 74, speed: 64 },
      { temp: 80, speed: 100 },
    ],
  },
}

const defaultGpuOverclock: GpuTuningState = {
  coreClock: 0,
  memoryClock: 0,
  voltageOffset: 0,
  powerLimit: 100,
  tempLimit: 83,
}

const nitroSenseExtremeGpuOverclock: GpuTuningState = {
  coreClock: 100,
  memoryClock: 200,
  voltageOffset: 0,
  powerLimit: 100,
  tempLimit: 83,
}

const builtInOcProfileSlots: OcProfileSlot[] = [
  {
    id: 'silent-uv',
    label: 'P1',
    name: 'Acer Quiet',
    strap: 'NitroSense quiet GPU tune',
    settings: {
      ...defaultGpuOverclock,
    },
  },
  {
    id: 'daily',
    label: 'P2',
    name: 'Acer Default',
    strap: 'NitroSense default GPU tune',
    settings: {
      ...defaultGpuOverclock,
    },
  },
  {
    id: 'creator',
    label: 'P3',
    name: 'Acer Performance',
    strap: 'NitroSense performance offset',
    settings: {
      ...nitroSenseExtremeGpuOverclock,
    },
  },
  {
    id: 'arena',
    label: 'P4',
    name: 'Acer Turbo',
    strap: 'NitroSense turbo offset',
    settings: {
      ...nitroSenseExtremeGpuOverclock,
    },
  },
]

const defaultCustomOcSlot: OcProfileSlot = {
  id: 'custom-user',
  label: 'P5',
  name: 'Custom Preset',
  strap: 'User-saved GPU tuning',
  settings: {
    ...defaultGpuOverclock,
  },
  isCustom: true,
}

function buildCustomOcStrap(settings: GpuTuningState) {
  const core = settings.coreClock >= 0 ? `+${settings.coreClock}` : `${settings.coreClock}`
  const memory =
    settings.memoryClock >= 0 ? `+${settings.memoryClock}` : `${settings.memoryClock}`
  const voltage =
    settings.voltageOffset >= 0
      ? `+${settings.voltageOffset}mV`
      : `${settings.voltageOffset}mV`

  return `Core ${core} / Mem ${memory} / ${voltage}`
}

const navigationTabs: { id: ControlTab; label: string }[] = [
  { id: 'home', label: 'Home' },
  { id: 'power', label: 'Power' },
  { id: 'fans', label: 'Fans' },
  { id: 'personal', label: 'Settings' },
  { id: 'debug', label: 'Debug' },
]

const personalSections: {
  id: PersonalSection
  label: string
  description: string
}[] = [
  {
    id: 'updates',
    label: 'Updates',
    description: 'Release checks, staged installs, and updater status.',
  },
  {
    id: 'charge',
    label: 'Battery & Charge',
    description: 'Battery preservation controls and charge target behavior.',
  },
  {
    id: 'screen',
    label: 'Screen',
    description: 'Display comfort controls and eye-care settings.',
  },
  {
    id: 'boot',
    label: 'System Boot Effect',
    description: 'Boot image preview, selection, and upload staging.',
  },
  {
    id: 'backup',
    label: 'Backup & Restore',
    description: 'Export or import profiles, curves, presets, and personal settings.',
  },
]

const fanTelemetryByProfile: Record<
  FanProfile['id'],
  { gpuRpm: number; cpuRpm: number; modeLabel: string }
> = {
  auto: { gpuRpm: 3385, cpuRpm: 3089, modeLabel: 'Automatic airflow mapping' },
  cool: { gpuRpm: 4240, cpuRpm: 3980, modeLabel: 'Cool adaptive fan curve' },
  max: { gpuRpm: 5110, cpuRpm: 4950, modeLabel: 'Full-speed thermal override' },
  custom: { gpuRpm: 3385, cpuRpm: 3089, modeLabel: 'Custom curve runtime' },
}

const BACKEND_POLL_INTERVAL_MS = 5000
const BACKEND_SECONDARY_POLL_INTERVAL_MS = 10000
const HIDDEN_BACKEND_POLL_INTERVAL_MS = 30000
const HARDWARE_READINESS_REFRESH_MS = 15_000
const RUNTIME_ESTIMATE_COUNTDOWN_SEC = 30
const PERFORMANCE_LOG_BATCH_SIZE = 16
const PERFORMANCE_LOG_FLUSH_DELAY_MS = 1500
const PERFORMANCE_LOG_LONG_FRAME_MS = 34
const PERFORMANCE_LOG_MAX_QUEUE = 200
const FAN_PROFILE_APPLY_TIMEOUT_MS = 15_000

function isDesktopRuntime() {
  return Boolean((window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__)
}

function isDevBuild() {
  return Boolean(import.meta.env.DEV)
}

function waitForNextPaint() {
  return new Promise<void>((resolve) => {
    if (
      typeof window.requestAnimationFrame !== 'function' ||
      document.visibilityState === 'hidden'
    ) {
      // rAF does not fire while hidden; never let an apply wait on a paint that
      // will not happen.
      window.setTimeout(resolve, 0)
      return
    }

    window.requestAnimationFrame(() => {
      window.requestAnimationFrame(() => resolve())
    })
  })
}

function formatStageFitValue(snapshot: StageFitSnapshot | undefined) {
  if (!snapshot) {
    return null
  }

  return `${snapshot.tab} ${snapshot.scale.toFixed(3)} / ${Math.round(snapshot.scaledHeight)}px`
}

function formatStageFitDetail(snapshot: StageFitSnapshot | undefined) {
  if (!snapshot) {
    return null
  }

  return `natural ${Math.round(snapshot.naturalWidth)}x${Math.round(
    snapshot.naturalHeight,
  )}, available ${Math.round(snapshot.availableWidth)}x${Math.round(
    snapshot.availableHeight,
  )}, window ${snapshot.windowWidth}x${snapshot.windowHeight} at ${snapshot.updatedAt}`
}

function hasUsableTelemetry(snapshot: TelemetrySnapshot | null | undefined) {
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

function describeTelemetrySource(
  serviceConnected: boolean,
  telemetry: TelemetrySnapshot | null | undefined,
) {
  if (serviceConnected) {
    return 'Service pipe'
  }

  return hasUsableTelemetry(telemetry) ? 'Cached service state' : 'No telemetry'
}

function describeFeatureSupport(
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

function buildHardwareReadinessTitle(snapshot: HardwareReadinessSnapshot) {
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

function formatLayerState(state: ControlLayerStatus['state']) {
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

function summarizeLayerAttention(statuses: ControlLayerStatus[] | null | undefined) {
  const attention = (statuses ?? []).filter((status) =>
    ['failed', 'unavailable', 'unknown'].includes(status.state),
  )
  if (attention.length === 0) {
    return null
  }

  return attention.map((status) => `${status.label}: ${formatLayerState(status.state)}`).join(' | ')
}

function buildCpuThermalSummary(
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

function buildPowerDashboardSummary(
  temperatureC: number | null | undefined,
  usagePercent: number | null | undefined,
) {
  const temperatureLabel = temperatureC != null ? `Temp ${temperatureC}C` : null
  const usageLabel = usagePercent != null ? `Usage ${usagePercent}%` : null

  return [temperatureLabel, usageLabel].filter(Boolean).join(' | ')
}

function buildCpuPowerDashboardSummary(
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

function buildPowerTargetFallback(
  profileId: PowerProfile['id'],
  customBase: CustomPowerBaseId,
  customProcessorState: { min: number; max: number },
  liveCpuPl2Label: string | null,
) {
  switch (profileId) {
    case 'battery-guard':
      return {
        label: '28W PL1 target',
        detail: 'Quiet maps CPU PL1 to 28W.',
      }
    case 'balanced':
      return {
        label: '45W PL1 target',
        detail: 'Balanced maps CPU PL1 to 45W.',
      }
    case 'performance':
      return {
        label: '75W PL1 target',
        detail: 'Performance maps CPU PL1 to 75W.',
      }
    case 'turbo':
      return {
        label: liveCpuPl2Label != null ? `${liveCpuPl2Label} PL1 target` : 'PL1 follows PL2',
        detail:
          liveCpuPl2Label != null
            ? `Turbo raises CPU PL1 to current PL2 (${liveCpuPl2Label}).`
            : 'Turbo raises CPU PL1 to current PL2 when RAPL readback is available.',
      }
    case 'custom':
      return {
        label: `${customPowerBaseCeilingLabels[customBase]} - ${Math.round(
          18 + customProcessorState.max * 0.57,
        )}W target`,
        detail: `Custom rides the ${customPowerBaseCeilingLabels[customBase].toLowerCase()}.`,
      }
  }
}

function buildGpuPowerDashboardSummary(
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

function getProcessorStateForPowerProfile(
  profileId: PowerProfile['id'],
  customProcessorState: { min: number; max: number },
) {
  switch (profileId) {
    case 'battery-guard':
      return { min: 5, max: 45 }
    case 'balanced':
      return { min: 35, max: 88 }
    case 'performance':
      return { min: 100, max: 100 }
    case 'turbo':
      return { min: 100, max: 100 }
    case 'custom':
    default:
      return customProcessorState
  }
}

function applyPowerControlSnapshot(
  controls: Pick<ControlSnapshot, 'activePowerProfile' | 'customProcessorState'>,
  setActivePowerProfile: (profile: PowerProfile['id']) => void,
  setCustomProcessorState: (state: { min: number; max: number }) => void,
) {
  setActivePowerProfile(controls.activePowerProfile)
  setCustomProcessorState({
    min: controls.customProcessorState.minPercent,
    max: controls.customProcessorState.maxPercent,
  })
}

function fromBackendGpuTuningState(
  tuning: ControlSnapshot['gpuTuning'],
): GpuTuningState {
  return {
    coreClock: tuning.coreClockMhz,
    memoryClock: tuning.memoryClockMhz,
    voltageOffset: tuning.voltageOffsetMv,
    powerLimit: tuning.powerLimitPercent,
    tempLimit: tuning.tempLimitC,
  }
}

function toBackendGpuTuningState(tuning: GpuTuningState): ControlSnapshot['gpuTuning'] {
  return {
    coreClockMhz: tuning.coreClock,
    memoryClockMhz: tuning.memoryClock,
    voltageOffsetMv: tuning.voltageOffset,
    powerLimitPercent: tuning.powerLimit,
    tempLimitC: tuning.tempLimit,
  }
}

function fromBackendOcPreset(preset: ControlSnapshot['ocPresets'][number]): OcProfileSlot {
  return {
    id: preset.id,
    label: preset.label,
    name: preset.name,
    strap: preset.strap,
    settings: fromBackendGpuTuningState(preset.settings),
    isCustom: preset.isCustom,
  }
}

function toBackendOcPreset(slot: OcProfileSlot): ControlSnapshot['ocPresets'][number] {
  return {
    id: slot.id,
    label: slot.label,
    name: slot.name,
    strap: slot.strap,
    settings: toBackendGpuTuningState(slot.settings),
    isCustom: Boolean(slot.isCustom),
  }
}

function toBackendCurveSet(curves: CurveSet): ControlSnapshot['fanCurves'] {
  const normalizedCurves = duplicateCurveSet(curves)
  return {
    cpu: normalizedCurves.cpu.map((point) => ({
      tempC: point.temp,
      speedPercent: point.speed,
    })),
    gpu: normalizedCurves.gpu.map((point) => ({
      tempC: point.temp,
      speedPercent: point.speed,
    })),
  }
}

function fromBackendCurveSet(curves: ControlSnapshot['fanCurves']): CurveSet {
  return duplicateCurveSet({
    cpu: curves.cpu.map((point) => ({
      temp: point.tempC,
      speed: point.speedPercent,
    })),
    gpu: curves.gpu.map((point) => ({
      temp: point.tempC,
      speed: point.speedPercent,
    })),
  })
}

function mergeControlsWithLiveSnapshot(
  controls: ControlSnapshot,
  liveControls: LiveControlSnapshot | null,
): ControlSnapshot {
  if (!liveControls) {
    return controls
  }

  return {
    ...controls,
    customProcessorState:
      controls.activePowerProfile === 'custom' &&
      liveControls.activePowerProfile === 'custom' &&
      liveControls.processorState
        ? liveControls.processorState
        : controls.customProcessorState,
    customPowerBase: controls.customPowerBase,
  }
}

function buildControlSnapshotForPersistence(input: {
  activePowerProfile: PowerProfile['id']
  activeFanProfile: FanProfile['id']
  customProcessorState: { min: number; max: number }
  customPowerBase: CustomPowerBaseId
  gpuOverclock: GpuTuningState
  ocProfileSlots: OcProfileSlot[]
  activeOcSlot: string
  ocApplyState: 'staged' | 'live'
  ocTuningLocked: boolean
  customCurves: CurveSet
  fanPresets: FanControlPreset[]
  powerPresets: PowerControlPreset[]
  quickPresets: QuickControlPreset[]
  fanSyncLockEnabled: boolean
  smartChargingEnabled: boolean
  processorStateControlEnabled: boolean
  nvidiaTelemetryEnabled: boolean
  autoRefreshRateOnBatteryEnabled: boolean
  autoRefreshRateRestoreHz: number | null
  usbPowerEnabled: boolean
  blueLightFilterEnabled: boolean
  selectedBootArt: string
  customBootFilename: string
  updateChannel: UpdateChannel
  checkForUpdatesOnLaunch: boolean
}): ControlSnapshot {
  return {
    activePowerProfile: input.activePowerProfile,
    activeFanProfile: input.activeFanProfile,
    customProcessorState: {
      minPercent: input.customProcessorState.min,
      maxPercent: input.customProcessorState.max,
    },
    customPowerBase: input.customPowerBase,
    gpuTuning: toBackendGpuTuningState(input.gpuOverclock),
    ocPresets: input.ocProfileSlots.map(toBackendOcPreset),
    activeOcSlot: input.activeOcSlot,
    ocApplyState: input.ocApplyState,
    ocTuningLocked: input.ocTuningLocked,
    fanCurves: toBackendCurveSet(input.customCurves),
    fanPresets: input.fanPresets,
    powerPresets: input.powerPresets,
    quickPresets: input.quickPresets,
    fanSyncLockEnabled: input.fanSyncLockEnabled,
    personalSettings: {
      smartChargingEnabled: input.smartChargingEnabled,
      usbPowerEnabled: input.usbPowerEnabled,
      processorStateControlEnabled: input.processorStateControlEnabled,
      nvidiaTelemetryEnabled: input.nvidiaTelemetryEnabled,
      blueLightFilterEnabled: input.blueLightFilterEnabled,
      autoRefreshRateOnBatteryEnabled: input.autoRefreshRateOnBatteryEnabled,
      autoRefreshRateRestoreHz: input.autoRefreshRateRestoreHz,
      selectedBootArt: input.selectedBootArt as ControlSnapshot['personalSettings']['selectedBootArt'],
      customBootFilename: input.customBootFilename,
      updateChannel: input.updateChannel,
      checkForUpdatesOnLaunch: input.checkForUpdatesOnLaunch,
    },
  }
}

function applyBackendControlSnapshot(
  controls: ControlSnapshot,
  setActivePowerProfile: (profile: PowerProfile['id']) => void,
  setCustomProcessorState: (state: { min: number; max: number }) => void,
  setCustomPowerBase: (base: CustomPowerBaseId) => void,
  setGpuOverclock: (state: GpuTuningState) => void,
  setCustomOcSlot: (slot: OcProfileSlot) => void,
  setActiveOcSlot: (slotId: string) => void,
  setOcApplyState: (state: 'staged' | 'live') => void,
  setOcTuningLocked: (locked: boolean) => void,
  setActiveFanProfile: (profile: FanProfile['id']) => void,
  setCustomCurves: (curves: CurveSet) => void,
  setFanSyncLockEnabled: (enabled: boolean) => void,
  setSmartChargingEnabled: (enabled: boolean) => void,
  setProcessorStateControlEnabled: (enabled: boolean) => void,
  setNvidiaTelemetryEnabled: (enabled: boolean) => void,
  setUsbPowerEnabled: (enabled: boolean) => void,
  setBlueLightFilterEnabled: (enabled: boolean) => void,
  setSelectedBootArt: (art: string) => void,
  setCustomBootFilename: (filename: string) => void,
  setUpdateChannel: (channel: UpdateChannel) => void,
  setCheckForUpdatesOnLaunch: (enabled: boolean) => void,
  setAutoRefreshRateSettings?: (enabled: boolean, restoreHz: number | null) => void,
  setPresetCollections?: (controls: ControlSnapshot) => void,
) {
  applyPowerControlSnapshot(controls, setActivePowerProfile, setCustomProcessorState)
  setCustomPowerBase(controls.customPowerBase)
  setGpuOverclock(fromBackendGpuTuningState(controls.gpuTuning))
  setActiveFanProfile(controls.activeFanProfile)
  setCustomCurves(fromBackendCurveSet(controls.fanCurves))
  setFanSyncLockEnabled(controls.fanSyncLockEnabled)

  const customSlot =
    controls.ocPresets.find((preset) => preset.isCustom) ?? toBackendOcPreset(defaultCustomOcSlot)
  const mappedCustomSlot = fromBackendOcPreset(customSlot)

  setCustomOcSlot(mappedCustomSlot)
  setActiveOcSlot(controls.activeOcSlot)
  setOcApplyState(controls.ocApplyState)
  setOcTuningLocked(controls.ocTuningLocked)
  setSmartChargingEnabled(controls.personalSettings.smartChargingEnabled)
  setProcessorStateControlEnabled(controls.personalSettings.processorStateControlEnabled)
  setNvidiaTelemetryEnabled(controls.personalSettings.nvidiaTelemetryEnabled ?? false)
  setUsbPowerEnabled(controls.personalSettings.usbPowerEnabled)
  setBlueLightFilterEnabled(controls.personalSettings.blueLightFilterEnabled)
  setAutoRefreshRateSettings?.(
    controls.personalSettings.autoRefreshRateOnBatteryEnabled,
    controls.personalSettings.autoRefreshRateRestoreHz,
  )
  setSelectedBootArt(controls.personalSettings.selectedBootArt)
  setCustomBootFilename(controls.personalSettings.customBootFilename)
  setUpdateChannel(controls.personalSettings.updateChannel ?? 'stable')
  setCheckForUpdatesOnLaunch(controls.personalSettings.checkForUpdatesOnLaunch)
  setPresetCollections?.(controls)
}

function App() {
  const dashboardRef = useRef<HTMLElement | null>(null)
  const topbarRef = useRef<HTMLElement | null>(null)
  const homeStageRef = useRef<HTMLElement | null>(null)
  const fansStageRef = useRef<HTMLElement | null>(null)
  const powerStageRef = useRef<HTMLElement | null>(null)
  const chartRefs = useRef<Record<CurveTarget, SVGSVGElement | null>>({
    cpu: null,
    gpu: null,
  })
  const backendPollInFlightRef = useRef(false)
  const controlApplyInFlightRef = useRef(0)
  const powerProfileApplyInFlightRef = useRef(false)
  const queuedPowerProfileRef = useRef<PowerProfile['id'] | null>(null)
  const fanProfileApplyInFlightRef = useRef(false)
  const queuedFanProfileRef = useRef<FanProfile['id'] | null>(null)
  const lastTransportDebugRef = useRef<string>('')
  const lastPollHeartbeatRef = useRef(0)
  const hardwareReadinessRefreshRef = useRef(0)
  const runtimeEstimateSessionRef = useRef(false)
  const [activeTab, setActiveTab] = useState<ControlTab>('home')
  const [activePowerProfile, setActivePowerProfile] =
    useState<PowerProfile['id']>('balanced')
  const [customProcessorState, setCustomProcessorState] = useState({
    min: 35,
    max: 88,
  })
  const customProcessorStateRef = useRef(customProcessorState)
  const [customPowerBase, setCustomPowerBase] = useState<CustomPowerBaseId>('performance')
  const customPowerBaseRef = useRef<CustomPowerBaseId>('performance')
  const customPowerApplyTimerRef = useRef<number | null>(null)
  const customPowerApplyRevisionRef = useRef(0)
  const controlsBootstrappedRef = useRef(false)
  const pulseTimerRef = useRef<number | null>(null)
  const [gpuOverclock, setGpuOverclock] = useState<GpuTuningState>(defaultGpuOverclock)
  const [customOcSlot, setCustomOcSlot] = useState<OcProfileSlot>(defaultCustomOcSlot)
  const [activeOcSlot, setActiveOcSlot] = useState<string>('daily')
  const [ocTuningLocked, setOcTuningLocked] = useState(false)
  const [ocApplyState, setOcApplyState] = useState<'staged' | 'live'>('live')
  const [activeFanProfile, setActiveFanProfile] =
    useState<FanProfile['id']>('auto')
  const [activePersonalSection, setActivePersonalSection] =
    useState<PersonalSection>('updates')
  const [customCurves, setCustomCurves] = useState<CurveSet>(
    duplicateCurveSet(presetCurves.custom),
  )
  const customCurvesRef = useRef<CurveSet>(duplicateCurveSet(presetCurves.custom))
  const [fanPresets, setFanPresets] = useState<FanControlPreset[]>([])
  const [powerPresets, setPowerPresets] = useState<PowerControlPreset[]>([])
  const [quickPresets, setQuickPresets] = useState<QuickControlPreset[]>([])
  const fanPresetsRef = useRef<FanControlPreset[]>([])
  const powerPresetsRef = useRef<PowerControlPreset[]>([])
  const quickPresetsRef = useRef<QuickControlPreset[]>([])
  const syncPresetCollectionsRef = useRef<(controls: ControlSnapshot) => void>(() => {})
  const [fanSyncLockEnabled, setFanSyncLockEnabled] = useState(false)
  const [draggingPoint, setDraggingPoint] = useState<DragState | null>(null)
  const [smartChargingEnabled, setSmartChargingEnabled] = useState(true)
  const smartChargingEnabledRef = useRef(true)
  const [processorStateControlEnabled, setProcessorStateControlEnabled] = useState(true)
  const processorStateControlEnabledRef = useRef(true)
  const [nvidiaTelemetryEnabled, setNvidiaTelemetryEnabledState] = useState(false)
  const nvidiaTelemetryEnabledRef = useRef(false)
  const [usbPowerEnabled, setUsbPowerEnabled] = useState(true)
  const [blueLightFilterEnabled, setBlueLightFilterEnabled] = useState(false)
  const blueLightFilterEnabledRef = useRef(false)
  const [autoRefreshRateOnBatteryEnabled, setAutoRefreshRateOnBatteryEnabled] =
    useState(false)
  const autoRefreshRateOnBatteryEnabledRef = useRef(false)
  const [autoRefreshRateRestoreHz, setAutoRefreshRateRestoreHz] = useState<number | null>(
    null,
  )
  const autoRefreshRateRestoreHzRef = useRef<number | null>(null)
  const autoRefreshRateSyncKeyRef = useRef<string | null>(null)
  const [selectedBootArt, setSelectedBootArt] = useState<string>('ember')
  const [customBootPreview, setCustomBootPreview] = useState<string | null>(null)
  const [customBootFilename, setCustomBootFilename] = useState<string>('custom-boot.png')
  const [updateChannel, setUpdateChannel] = useState<UpdateChannel>('stable')
  const [checkForUpdatesOnLaunch, setCheckForUpdatesOnLaunch] = useState(true)
  const [backendCapabilities, setBackendCapabilities] = useState<CapabilitySnapshot | null>(null)
  const [backendVersion, setBackendVersion] = useState(__APP_VERSION__)
  const [updateStatus, setUpdateStatus] = useState<UpdateStatus | null>(null)
  const [updateActionPending, setUpdateActionPending] = useState<UpdateAction | null>(null)
  const [updateActionMessage, setUpdateActionMessage] = useState<string | null>(null)
  const autoUpdateCheckTriggeredRef = useRef(false)
  const updateNotificationKeyRef = useRef<string | null>(null)
  const [statusMessage, setStatusMessage] = useState(
    'Desktop backend starting. Loading persisted AeroForge state.',
  )
  const [toasts, setToasts] = useState<AppToast[]>([])
  const toastTimersRef = useRef<Record<string, number>>({})
  const toastSequenceRef = useRef(0)
  const [settingsActionPending, setSettingsActionPending] =
    useState<SettingsActionPending>(null)
  const settingsActionPendingRef = useRef<SettingsActionPending>(null)
  const [glowTarget, setGlowTarget] = useState<string>('turbo')
  const [shellStatus, setShellStatus] = useState('Browser preview shell')
  const [serviceConnected, setServiceConnected] = useState(false)
  const serviceConnectedRef = useRef(false)
  const [serviceStatus, setServiceStatus] = useState<ServiceStatus | null>(null)
  const [hardwareReadiness, setHardwareReadiness] =
    useState<HardwareReadinessSnapshot | null>(null)
  const autoServiceHealKeyRef = useRef<string | null>(null)
  const maybeRequestBackgroundServiceHealRef = useRef<
    (snapshot: HardwareReadinessSnapshot) => void
  >(() => {})
  const refreshHardwareReadinessRef = useRef<(force?: boolean) => Promise<void>>(async () => {})
  const [liveTelemetry, setLiveTelemetry] = useState<TelemetrySnapshot | null>(null)
  const [telemetrySourceLabel, setTelemetrySourceLabel] = useState('No telemetry')
  const [lastBackendPollAt, setLastBackendPollAt] = useState<string>('Waiting')
  const [lastBackendError, setLastBackendError] = useState<string | null>(null)
  const [debugEvents, setDebugEvents] = useState<string[]>([])
  const [performanceLogState, setPerformanceLogState] = useState<PerformanceLogState>({
    path: null,
    lastFlushAt: 'Waiting',
    pendingCount: 0,
    eventCount: 0,
    lastError: null,
  })
  const [frameStats, setFrameStats] = useState<FrameStats>({
    averageMs: 0,
    maxMs: 0,
    fps: 0,
    longFrameCount: 0,
    sampleWindowMs: 0,
    updatedAt: 'Waiting',
  })
  const [homeScale, setHomeScale] = useState(1)
  const [homeScaledHeight, setHomeScaledHeight] = useState<number | null>(null)
  const [fansScale, setFansScale] = useState(1)
  const [fansScaledHeight, setFansScaledHeight] = useState<number | null>(null)
  const [powerScale, setPowerScale] = useState(1)
  const [powerScaledHeight, setPowerScaledHeight] = useState<number | null>(null)
  const [stageFitSnapshots, setStageFitSnapshots] = useState<
    Partial<Record<StageFitTarget, StageFitSnapshot>>
  >({})
  const [runtimeEstimateCountdownSec, setRuntimeEstimateCountdownSec] = useState(0)
  const initializedPersistenceRef = useRef(false)
  const stagedPersistenceTimerRef = useRef<number | null>(null)
  const activeTabRef = useRef(activeTab)
  const debugEventsRef = useRef<string[]>([])
  const stageFitSignatureRef = useRef<Partial<Record<StageFitTarget, string>>>({})
  const stageFitSnapshotsRef = useRef<Partial<Record<StageFitTarget, StageFitSnapshot>>>({})
  const telemetrySnapshotRef = useRef<string | null>(null)
  const liveControlSnapshotStateRef = useRef<string | null>(null)
  const liveControlSnapshotRef = useRef<LiveControlSnapshot | null>(null)
  const [liveControlSnapshot, setLiveControlSnapshot] =
    useState<LiveControlSnapshot | null>(null)
  const debugServiceStatusRef = useRef<string | null>(null)
  const performanceLogSessionIdRef = useRef(`af-${Date.now().toString(36)}`)
  const performanceLogQueueRef = useRef<PerformanceLogEvent[]>([])
  const performanceLogFlushTimerRef = useRef<number | null>(null)
  const performanceLogFlushInFlightRef = useRef(false)
  const performanceLogEventCountRef = useRef(0)
  const performanceLogPathRef = useRef<string | null>(null)
  const performanceLogLastErrorRef = useRef<string | null>(null)
  const queuePerformanceEventRef = useRef<
    (eventType: string, detail: string, payload?: Record<string, unknown>) => void
  >(() => {})
  const flushPerformanceLogRef = useRef<() => Promise<void>>(async () => {})
  const syncAutoRefreshRateStateRef = useRef<
    (enabled: boolean, onBattery: boolean, announce: boolean) => Promise<unknown>
  >(async () => null)
  const persistStagedControlsRef = useRef<
    (overrides?: PersistControlOverrides) => Promise<void>
  >(async () => {})
  const refreshCachedLiveControlsAfterApplyRef = useRef<
    () => Promise<{ liveControls: LiveControlSnapshot | null; refreshMs: number }>
  >(async () => ({ liveControls: null, refreshMs: 0 }))
  const finalizeCustomCurveEditRef = useRef<
    (nextCurves: CurveSet, options?: FinalizeCustomCurveOptions) => Promise<void>
  >(async () => {})
  const handlePowerProfileRef = useRef<(profileId: PowerProfile['id']) => Promise<void>>(
    async () => {},
  )
  const handleFanProfileRef = useRef<(profileId: FanProfile['id']) => Promise<void>>(
    async () => {},
  )
  const presetImportInputRef = useRef<HTMLInputElement | null>(null)
  const copyDiagnosticsToClipboardRef = useRef<() => Promise<void>>(async () => {})
  const exportDiagnosticsFileRef = useRef<() => void>(() => {})
  const exportSettingsBackupRef = useRef<() => void>(() => {})
  const dismissNewestToastRef = useRef<() => void>(() => {})
  const settingsImportInputRef = useRef<HTMLInputElement | null>(null)
  const [shortcutsHelpOpen, setShortcutsHelpOpen] = useState(false)
  const pushDebugEventRef = useRef<(message: string) => void>(() => {})
  const pushTransportDebugEventRef = useRef<(message: string) => void>(() => {})
  const pushPollHeartbeatEventRef = useRef<(message: string) => void>(() => {})
  const runUpdateCheckRef = useRef<
    ((manual: boolean, channelOverride?: UpdateChannel) => Promise<UpdateStatus | null>) | null
  >(null)

  const activePreset =
    activeFanProfile === 'custom' ? customCurves : presetCurves[activeFanProfile]
  const fanTelemetryDescriptor = fanTelemetryByProfile[activeFanProfile]
  const smartChargeTarget = smartChargingEnabled ? '80%' : '100%'
  const smartChargePending = settingsActionPending === 'smart-charge'
  const blueLightPending = settingsActionPending === 'blue-light'
  const bootLogoPending = settingsActionPending === 'boot-logo'
  const refreshRatePending = settingsActionPending === 'refresh-rate'
  const nvidiaTelemetryPending = settingsActionPending === 'nvidia-telemetry'
  const hardwareRepairPending = settingsActionPending === 'hardware-repair'
  const hardwareStartPending = settingsActionPending === 'hardware-start'
  const hardwareIssueVisible = hardwareReadiness !== null && !hardwareReadiness.ready
  const hardwareIssueTitle = hardwareReadiness
    ? buildHardwareReadinessTitle(hardwareReadiness)
    : 'Hardware readiness unknown'
  const hardwareIssueDetail = hardwareReadiness?.detail ?? 'Waiting for hardware diagnostics.'
  const powerLayerStatuses = liveControlSnapshot?.lastPowerLayerStatuses ?? []
  const fanLayerStatuses = liveControlSnapshot?.lastFanLayerStatuses ?? []
  const powerHardwareApplyReady = serviceConnected && (hardwareReadiness?.ready ?? true)
  const fanHardwareApplyReady = serviceConnected && (hardwareReadiness?.ready ?? true)
  const currentPowerProfile = powerProfiles.find(
    (profile) => profile.id === activePowerProfile,
  )!
  const currentCustomPowerBase =
    customPowerBaseOptions.find((option) => option.id === customPowerBase) ??
    customPowerBaseOptions[1]
  const currentFanProfile = fanProfiles.find((profile) => profile.id === activeFanProfile)!
  const currentBootArt = bootArtwork.find((art) => art.id === selectedBootArt)
  const smartChargeDisabledReason = describeFeatureSupport(
    backendCapabilities?.smartCharging,
    'Battery-health charging control is not available on this machine.',
  )
  const usbPowerDisabledReason = describeFeatureSupport(
    backendCapabilities?.usbPower,
    'Power-off USB charging is not available on this machine.',
    'Power-off USB charging is still not wired to a verified Windows hardware path in AeroForge.',
  )
  const blueLightDisabledReason = describeFeatureSupport(
    backendCapabilities?.blueLightFilter,
    'AeroForge could not expose the blue light filter on this machine.',
  )
  const bootLogoDisabledReason = describeFeatureSupport(
    backendCapabilities?.bootLogo,
    'Boot-logo apply requires the AeroForge service and an unambiguous EFI System Partition.',
  )
  const smartChargeWritable = !smartChargeDisabledReason
  const usbPowerVisible = backendCapabilities?.usbPower.available ?? false
  const usbPowerWritable = !usbPowerDisabledReason
  const blueLightWritable = !blueLightDisabledReason
  const bootLogoWritable = !bootLogoDisabledReason
  const bootLogoStatusText = bootLogoWritable
    ? null
    : bootLogoDisabledReason
  const runtimeCustomOcSlot =
    activeOcSlot === customOcSlot.id
      ? {
          ...customOcSlot,
          strap: buildCustomOcStrap(gpuOverclock),
          settings: { ...gpuOverclock },
        }
      : customOcSlot
  const ocProfileSlots = [...builtInOcProfileSlots, runtimeCustomOcSlot]
  const currentOcSlot = ocProfileSlots.find((slot) => slot.id === activeOcSlot)!
  const activeTelemetry = hasUsableTelemetry(liveTelemetry) ? liveTelemetry : null
  const displayedAcPluggedIn = activeTelemetry?.acPluggedIn ?? null
  const displayedCpuTemp = presentPositive(
    activeTelemetry?.cpuTempAverageC ?? activeTelemetry?.cpuTempC ?? null,
  )
  const displayedCpuTempLowest = activeTelemetry?.cpuTempLowestCoreC ?? null
  const displayedCpuTempHighest = activeTelemetry?.cpuTempHighestCoreC ?? null
  const displayedGpuTemp = presentPositive(activeTelemetry?.gpuTempC ?? null)
  const displayedSystemTemp = presentPositive(activeTelemetry?.systemTempC ?? null)
  const displayedBatteryPercent = activeTelemetry?.batteryPercent ?? null
  const displayedBatteryLifeRemainingSec = activeTelemetry?.batteryLifeRemainingSec ?? null
  const displayedCpuUsage = activeTelemetry?.cpuUsagePercent ?? null
  const displayedGpuUsage = activeTelemetry?.gpuUsagePercent ?? null
  const displayedGpuMemoryUsage = activeTelemetry?.gpuMemoryUsagePercent ?? null
  const displayedGpuPowerDraw = activeTelemetry?.gpuPowerDrawW ?? null
  const displayedGpuPowerLimit = activeTelemetry?.gpuPowerLimitW ?? null
  const displayedGpuPowerDefaultLimit = activeTelemetry?.gpuPowerDefaultLimitW ?? null
  const displayedGpuPowerMaxLimit = activeTelemetry?.gpuPowerMaxLimitW ?? null
  const displayedCpuPackagePower = activeTelemetry?.cpuPackagePowerW ?? null
  const displayedCpuPl1 = activeTelemetry?.cpuPl1W ?? null
  const displayedCpuPl1Enabled = activeTelemetry?.cpuPl1Enabled ?? null
  const displayedCpuPl2 = activeTelemetry?.cpuPl2W ?? null
  const displayedCpuPl2Enabled = activeTelemetry?.cpuPl2Enabled ?? null
  const displayedCpuPowerLimitLocked = activeTelemetry?.cpuPowerLimitLocked ?? null
  const displayedGpuClock = presentPositive(activeTelemetry?.gpuClockMhz ?? null)
  const displayedCpuClock = presentPositive(activeTelemetry?.cpuClockMhz ?? null)
  const displayedCpuFanRpm = presentPositive(activeTelemetry?.cpuFanRpm ?? null)
  const displayedGpuFanRpm = presentPositive(activeTelemetry?.gpuFanRpm ?? null)
  const displayedCpuFanTargetPercent =
    liveControlSnapshot?.currentCpuFanSpeedPercent ??
    interpolateCurveTarget(activePreset.cpu, displayedCpuTemp)
  const displayedGpuFanTargetPercent =
    liveControlSnapshot?.currentGpuFanSpeedPercent ??
    interpolateCurveTarget(activePreset.gpu, displayedGpuTemp)
  const powerHeadline = currentPowerProfile.name
  const displayedCpuIdentity = buildHardwareIdentity(
    activeTelemetry?.cpuBrand,
    activeTelemetry?.cpuName,
    'CPU sensor',
  )
  const displayedGpuIdentity = buildHardwareIdentity(
    activeTelemetry?.gpuBrand,
    activeTelemetry?.gpuName,
    'GPU sensor',
  )
  const displayedSystemIdentity = buildHardwareIdentity(
    activeTelemetry?.systemVendor,
    activeTelemetry?.systemModel,
    'System sensor',
  )
  const liveGpuPowerLimitLabel = formatWattValue(displayedGpuPowerLimit, 1)
  const liveGpuPowerDrawLabel = formatWattValue(displayedGpuPowerDraw, 1)
  const liveCpuPackagePowerLabel = formatWattValue(displayedCpuPackagePower, 1)
  const liveCpuPl1Label = formatWattValue(displayedCpuPl1, 1)
  const liveCpuPl2Label = formatWattValue(displayedCpuPl2, 1)
  const liveCpuPowerLimitLabel = buildCpuPowerLimitLabel(liveCpuPl1Label, liveCpuPl2Label)
  const liveGpuPowerLimitShort = buildGpuPowerLimitLabel(
    liveGpuPowerLimitLabel,
    displayedGpuPowerMaxLimit,
  )
  const cpuPowerReadoutValue = liveCpuPackagePowerLabel ?? '--'
  const gpuPowerReadoutValue = liveGpuPowerDrawLabel ?? '--'
  const liveCpuPowerStateDetail =
    liveCpuPowerLimitLabel != null
      ? `${liveCpuPackagePowerLabel ? `Power ${liveCpuPackagePowerLabel}. ` : ''}${
          displayedCpuPowerLimitLocked ? 'Package limits locked.' : 'Package limits unlocked.'
        }${
          displayedCpuPl1Enabled === false || displayedCpuPl2Enabled === false
            ? ' One or more limits are disabled.'
            : ''
        }`
      : liveCpuPackagePowerLabel != null
        ? `CPU power ${liveCpuPackagePowerLabel}`
        : null
  const liveGpuPowerCeilingDetail =
    liveGpuPowerDrawLabel != null && liveGpuPowerLimitLabel != null
      ? `Draw ${liveGpuPowerDrawLabel} / Limit ${liveGpuPowerLimitLabel}`
      : liveGpuPowerLimitLabel != null
        ? `Limit ${liveGpuPowerLimitLabel}`
        : liveGpuPowerDrawLabel != null
          ? `Draw ${liveGpuPowerDrawLabel}`
          : displayedGpuPowerDefaultLimit != null && displayedGpuPowerMaxLimit != null
            ? `Default ${formatWattValue(displayedGpuPowerDefaultLimit, 1)} / Max ${formatWattValue(displayedGpuPowerMaxLimit, 1)}`
            : displayedGpuPowerMaxLimit != null
              ? `Driver max ${formatWattValue(displayedGpuPowerMaxLimit, 1)}`
              : null
  const livePowerCeilingDetail = [
    liveCpuPowerStateDetail,
    liveGpuPowerCeilingDetail ? `GPU ${liveGpuPowerCeilingDetail}` : null,
  ]
    .filter(Boolean)
    .join(' | ')
  const fallbackPowerTarget = buildPowerTargetFallback(
    activePowerProfile,
    customPowerBase,
    customProcessorState,
    liveCpuPl2Label,
  )
  const currentPowerWattage = `CPU ${cpuPowerReadoutValue} / GPU ${gpuPowerReadoutValue}`
  const currentPowerLimitDetail = [
    liveCpuPowerLimitLabel ? `CPU ${liveCpuPowerLimitLabel}` : null,
    liveGpuPowerLimitShort ? `GPU ${liveGpuPowerLimitShort}` : null,
  ]
    .filter(Boolean)
    .join(' | ')
  const cpuPowerMeterDetail = liveCpuPowerLimitLabel ?? 'CPU power driver missing'
  const gpuPowerMeterDetail = liveGpuPowerLimitShort ?? 'No readback'
  const currentPowerRuntime = formatRemainingRuntime(displayedBatteryLifeRemainingSec)
  const runtimeEstimatePending =
    serviceConnected &&
    displayedAcPluggedIn === false &&
    !currentPowerRuntime &&
    runtimeEstimateCountdownSec > 0
  const currentPowerRuntimeValue =
    runtimeEstimatePending
      ? `ETA ${runtimeEstimateCountdownSec}s`
      : currentPowerRuntime || (serviceConnected && displayedAcPluggedIn ? 'AC' : '')
  const currentPowerRuntimeDetail = !serviceConnected
    ? 'No live runtime estimate'
    : displayedAcPluggedIn
      ? 'Runtime unavailable while charging'
      : runtimeEstimatePending
        ? 'Calculating power draw'
      : currentPowerRuntime
        ? 'Live estimate from Windows'
        : 'Windows did not provide a runtime estimate'
  const currentPowerManagerDetail =
    currentPowerLimitDetail ||
    livePowerCeilingDetail ||
    fallbackPowerTarget.detail ||
    currentPowerRuntime ||
    currentPowerRuntimeDetail
  const currentPowerSummary =
    activePowerProfile === 'custom'
      ? processorStateControlEnabled
        ? `Processor state tuned to ${customProcessorState.min}% minimum and ${customProcessorState.max}% maximum on the ${currentCustomPowerBase.name} firmware base.`
        : `Custom rides the ${currentCustomPowerBase.name} firmware base. CPU min/max writes are disabled.`
      : activePowerProfile === 'battery-guard'
        ? processorStateControlEnabled
          ? 'Direct Whisper quiet-mode request with a conservative processor policy for lower noise and heat.'
          : 'Direct Whisper quiet-mode request without changing Windows CPU min/max state.'
        : activePowerProfile === 'performance'
          ? processorStateControlEnabled
            ? 'Direct Acer performance-mode apply with processor state pinned to 100% minimum and maximum.'
            : 'Direct Acer performance-mode apply without changing Windows CPU min/max state.'
        : activePowerProfile === 'turbo'
          ? processorStateControlEnabled
            ? 'Direct Acer turbo mode apply with processor state pinned to 100% minimum and maximum.'
            : 'Direct Acer turbo mode apply without changing Windows CPU min/max state.'
          : currentPowerProfile.summary
  const updateLastCheckedLabel = updateStatus?.lastCheckedAtUnix
    ? formatUnixClock(updateStatus.lastCheckedAtUnix)
    : 'Never'
  const updateLatestLabel =
    updateStatus?.latestVersion ??
    updateStatus?.latestCommitSha ??
    (updateStatus?.feedKind === 'none' ? 'Not published' : 'Waiting')
  const updateAvailabilityLabel = updateStatus?.updateAvailable
    ? 'Update ready'
    : updateStatus?.canInstallUpdate
      ? 'Staged update ready'
      : updateStatus?.feedKind === 'none'
        ? 'No release yet'
        : 'Up to date'
  const updateActionLabel =
    updateActionPending === 'check'
      ? 'Checking release feed'
      : updateActionPending === 'stage'
        ? 'Downloading update'
        : updateActionPending === 'install'
          ? 'Launching updater'
          : updateAvailabilityLabel
  const updateStatusDetail =
    updateActionMessage ?? updateStatus?.detail ?? 'Updater not checked yet.'
  const updateCheckButtonLabel =
    updateActionPending === 'check' ? 'Checking...' : 'Check for Updates'
  const updateDownloadButtonLabel =
    updateActionPending === 'stage' ? 'Downloading...' : 'Download Latest Update'
  const updateInstallButtonLabel =
    updateActionPending === 'install' ? 'Launching...' : 'Install Staged Update'
  useEffect(() => {
    const previousTab = activeTabRef.current
    activeTabRef.current = activeTab
    if (previousTab !== activeTab) {
      queuePerformanceEventRef.current('tab-change', `${previousTab} -> ${activeTab}`, {
        from: previousTab,
        to: activeTab,
        windowWidth: window.innerWidth,
        windowHeight: window.innerHeight,
        stageFit: stageFitSnapshotsRef.current[activeTab as StageFitTarget] ?? null,
      })
    }
    if (activeTab === 'debug') {
      setDebugEvents([...debugEventsRef.current])
    }
  }, [activeTab])

  useEffect(() => {
    stageFitSnapshotsRef.current = stageFitSnapshots
  }, [stageFitSnapshots])

  useEffect(() => {
    settingsActionPendingRef.current = settingsActionPending
  }, [settingsActionPending])

  function commitCustomCurves(next: CurveSet | ((current: CurveSet) => CurveSet)) {
    // Resolve against the ref, not inside the updater: pointerup reads the ref
    // synchronously and React may defer the updater past it during a drag.
    const resolved = typeof next === 'function' ? next(customCurvesRef.current) : next
    const normalized = duplicateCurveSet(resolved)
    customCurvesRef.current = normalized
    setCustomCurves(normalized)
  }

  type SnapshotScope = 'power' | 'fanProfile' | 'fanCurves' | 'gpu' | 'bootLogo' | 'nvidiaTelemetry'

  /**
   * Apply only the fields a backend response is authoritative for. The backend
   * builds every response from its last persisted snapshot, so applying the
   * whole thing would revert any edit the user made elsewhere while the request
   * was in flight (and the staged persist would then write that reversion).
   */
  function applyAuthoritativeSnapshot(controls: ControlSnapshot, scope: SnapshotScope) {
    switch (scope) {
      case 'power':
        applyPowerControlSnapshot(controls, setActivePowerProfile, setCustomProcessorState)
        setCustomPowerBase(controls.customPowerBase)
        commitProcessorStateControlEnabled(controls.personalSettings.processorStateControlEnabled)
        break
      case 'fanProfile':
        setActiveFanProfile(controls.activeFanProfile)
        break
      case 'fanCurves':
        setActiveFanProfile(controls.activeFanProfile)
        commitCustomCurves(fromBackendCurveSet(controls.fanCurves))
        break
      case 'gpu': {
        setGpuOverclock(fromBackendGpuTuningState(controls.gpuTuning))
        const customSlot =
          controls.ocPresets.find((preset) => preset.isCustom) ??
          toBackendOcPreset(defaultCustomOcSlot)
        setCustomOcSlot(fromBackendOcPreset(customSlot))
        setActiveOcSlot(controls.activeOcSlot)
        setOcApplyState(controls.ocApplyState)
        setOcTuningLocked(controls.ocTuningLocked)
        break
      }
      case 'bootLogo':
        setSelectedBootArt(controls.personalSettings.selectedBootArt)
        setCustomBootFilename(controls.personalSettings.customBootFilename)
        break
      case 'nvidiaTelemetry':
        commitNvidiaTelemetryEnabled(controls.personalSettings.nvidiaTelemetryEnabled ?? false)
        break
    }
  }

  function commitFanPresets(next: FanControlPreset[] | ((current: FanControlPreset[]) => FanControlPreset[])) {
    setFanPresets((current) => {
      const resolved =
        typeof next === 'function'
          ? (next as (current: FanControlPreset[]) => FanControlPreset[])(current)
          : next
      fanPresetsRef.current = resolved
      return resolved
    })
  }

  function commitPowerPresets(
    next: PowerControlPreset[] | ((current: PowerControlPreset[]) => PowerControlPreset[]),
  ) {
    setPowerPresets((current) => {
      const resolved =
        typeof next === 'function'
          ? (next as (current: PowerControlPreset[]) => PowerControlPreset[])(current)
          : next
      powerPresetsRef.current = resolved
      return resolved
    })
  }

  function commitQuickPresets(
    next: QuickControlPreset[] | ((current: QuickControlPreset[]) => QuickControlPreset[]),
  ) {
    setQuickPresets((current) => {
      const resolved =
        typeof next === 'function'
          ? (next as (current: QuickControlPreset[]) => QuickControlPreset[])(current)
          : next
      quickPresetsRef.current = resolved
      return resolved
    })
  }

  function syncPresetCollections(controls: ControlSnapshot) {
    commitFanPresets(controls.fanPresets ?? [])
    commitPowerPresets(controls.powerPresets ?? [])
    commitQuickPresets(controls.quickPresets ?? [])
  }
  syncPresetCollectionsRef.current = syncPresetCollections

  function maybeRequestBackgroundServiceHeal(snapshot: HardwareReadinessSnapshot) {
    if (snapshot.ready || !snapshot.service.installed || snapshot.service.running) {
      return
    }
    if (settingsActionPendingRef.current !== null) {
      return
    }

    const healKey = [
      snapshot.service.name,
      snapshot.service.startType ?? 'unknown',
      snapshot.wmi.service.running ? 'wmi-running' : 'wmi-stopped',
      snapshot.wmi.disabled ? 'wmi-disabled' : 'wmi-enabled',
      snapshot.repairAvailable ? 'repair-available' : 'repair-missing',
    ].join(':')
    if (autoServiceHealKeyRef.current === healKey) {
      return
    }

    autoServiceHealKeyRef.current = healKey
    settingsActionPendingRef.current = 'hardware-start'
    setSettingsActionPending('hardware-start')
    setStatusMessage('Starting AeroForge service in the background...')
    const toastId = showToast(
      'info',
      'Starting service',
      'AeroForge is using the background healer so normal launch stays non-admin.',
      0,
    )

    void requestServiceHeal()
      .then((result) => {
        setStatusMessage(result.detail)
        updateToast(
          toastId,
          result.serviceRunning ? 'success' : result.requested ? 'warning' : 'error',
          result.serviceRunning
            ? 'Service started'
            : result.requested
              ? 'Still starting'
              : 'Repair needed',
          result.detail,
        )
        pushDebugEventRef.current(result.detail)

        window.setTimeout(
          () => void refreshHardwareReadiness(true),
          result.serviceRunning ? 1000 : 5000,
        )
      })
      .catch((error) => {
        const detail = describeError(error)
        setStatusMessage(`AeroForge service start failed: ${detail}`)
        updateToast(toastId, 'error', 'Service start failed', detail)
        pushDebugEventRef.current(`background service heal failed: ${detail}`)
      })
      .finally(() => {
        settingsActionPendingRef.current = null
        setSettingsActionPending((current) => (current === 'hardware-start' ? null : current))
      })
  }
  maybeRequestBackgroundServiceHealRef.current = maybeRequestBackgroundServiceHeal

  function commitSmartChargingEnabled(enabled: boolean) {
    smartChargingEnabledRef.current = enabled
    setSmartChargingEnabled(enabled)
  }

  function commitProcessorStateControlEnabled(enabled: boolean) {
    processorStateControlEnabledRef.current = enabled
    setProcessorStateControlEnabled(enabled)
  }

  function commitNvidiaTelemetryEnabled(enabled: boolean) {
    nvidiaTelemetryEnabledRef.current = enabled
    setNvidiaTelemetryEnabledState(enabled)
  }

  function commitBlueLightFilterEnabled(enabled: boolean) {
    blueLightFilterEnabledRef.current = enabled
    setBlueLightFilterEnabled(enabled)
  }

  function commitAutoRefreshRateSettings(enabled: boolean, restoreHz: number | null) {
    autoRefreshRateOnBatteryEnabledRef.current = enabled
    autoRefreshRateRestoreHzRef.current = restoreHz
    setAutoRefreshRateOnBatteryEnabled(enabled)
    setAutoRefreshRateRestoreHz(restoreHz)
  }

  function updateSerializedState<T>(
    ref: { current: string | null },
    value: T | null,
    setter: (next: T | null) => void,
  ) {
    const serialized = value == null ? null : JSON.stringify(value)
    if (ref.current === serialized) {
      return
    }

    ref.current = serialized
    setter(value)
  }

  useEffect(() => {
    customProcessorStateRef.current = customProcessorState
  }, [customProcessorState])

  useEffect(() => {
    customPowerBaseRef.current = customPowerBase
  }, [customPowerBase])

  useEffect(() => {
    serviceConnectedRef.current = serviceConnected
  }, [serviceConnected])

  useEffect(
    () => () => {
      if (customPowerApplyTimerRef.current !== null) {
        window.clearTimeout(customPowerApplyTimerRef.current)
      }
      if (stagedPersistenceTimerRef.current !== null) {
        window.clearTimeout(stagedPersistenceTimerRef.current)
      }
      Object.values(toastTimersRef.current).forEach((timer) => window.clearTimeout(timer))
      toastTimersRef.current = {}
    },
    [],
  )

  async function persistStagedControls(overrides?: PersistControlOverrides) {
    if (!controlsBootstrappedRef.current) {
      // UI state is still the built-in defaults; saving now would overwrite the
      // user's presets and curves on disk with those defaults.
      pushDebugEventRef.current('staged control persistence skipped: backend controls not loaded yet')
      return
    }
    const nextActivePowerProfile = overrides?.activePowerProfile ?? activePowerProfile
    const nextActiveFanProfile = overrides?.activeFanProfile ?? activeFanProfile
    const nextCustomProcessorState =
      overrides?.customProcessorState ?? customProcessorStateRef.current
    const nextCustomPowerBase = overrides?.customPowerBase ?? customPowerBaseRef.current
    const nextCustomCurves = overrides?.customCurves ?? customCurvesRef.current
    const nextFanPresets = overrides?.fanPresets ?? fanPresetsRef.current
    const nextPowerPresets = overrides?.powerPresets ?? powerPresetsRef.current
    const nextQuickPresets = overrides?.quickPresets ?? quickPresetsRef.current
    const nextFanSyncLockEnabled = overrides?.fanSyncLockEnabled ?? fanSyncLockEnabled
    const nextSmartChargingEnabled =
      overrides?.smartChargingEnabled ?? smartChargingEnabledRef.current
    const nextProcessorStateControlEnabled =
      overrides?.processorStateControlEnabled ?? processorStateControlEnabledRef.current
    const nextNvidiaTelemetryEnabled =
      overrides?.nvidiaTelemetryEnabled ?? nvidiaTelemetryEnabledRef.current
    const nextAutoRefreshRateOnBatteryEnabled =
      overrides?.autoRefreshRateOnBatteryEnabled ??
      autoRefreshRateOnBatteryEnabledRef.current
    const nextAutoRefreshRateRestoreHz =
      overrides?.autoRefreshRateRestoreHz ?? autoRefreshRateRestoreHzRef.current
    const nextBlueLightFilterEnabled =
      overrides?.blueLightFilterEnabled ?? blueLightFilterEnabledRef.current
    const nextSelectedBootArt = overrides?.selectedBootArt ?? selectedBootArt
    const nextCustomBootFilename = overrides?.customBootFilename ?? customBootFilename
    const nextUpdateChannel = overrides?.updateChannel ?? updateChannel
    const nextCheckForUpdatesOnLaunch =
      overrides?.checkForUpdatesOnLaunch ?? checkForUpdatesOnLaunch

    try {
      await saveControlSnapshot(
        buildControlSnapshotForPersistence({
          activePowerProfile: nextActivePowerProfile,
          activeFanProfile: nextActiveFanProfile,
          customProcessorState: nextCustomProcessorState,
          customPowerBase: nextCustomPowerBase,
          gpuOverclock,
          ocProfileSlots,
          activeOcSlot,
          ocApplyState,
          ocTuningLocked,
          customCurves: nextCustomCurves,
          fanPresets: nextFanPresets,
          powerPresets: nextPowerPresets,
          quickPresets: nextQuickPresets,
          fanSyncLockEnabled: nextFanSyncLockEnabled,
          smartChargingEnabled: nextSmartChargingEnabled,
          processorStateControlEnabled: nextProcessorStateControlEnabled,
          nvidiaTelemetryEnabled: nextNvidiaTelemetryEnabled,
          autoRefreshRateOnBatteryEnabled: nextAutoRefreshRateOnBatteryEnabled,
          autoRefreshRateRestoreHz: nextAutoRefreshRateRestoreHz,
          usbPowerEnabled,
          blueLightFilterEnabled: nextBlueLightFilterEnabled,
          selectedBootArt: nextSelectedBootArt,
          customBootFilename: nextCustomBootFilename,
          updateChannel: nextUpdateChannel,
          checkForUpdatesOnLaunch: nextCheckForUpdatesOnLaunch,
        }),
      )
    } catch (error) {
      pushDebugEvent(`staged control persistence failed: ${describeError(error)}`)
    }
  }
  persistStagedControlsRef.current = persistStagedControls

  function scheduleStagedControlsPersist() {
    if (stagedPersistenceTimerRef.current !== null) {
      window.clearTimeout(stagedPersistenceTimerRef.current)
    }

    stagedPersistenceTimerRef.current = window.setTimeout(() => {
      stagedPersistenceTimerRef.current = null
      void persistStagedControlsRef.current()
    }, 450)
  }

  function flushStagedControlsPersist() {
    if (stagedPersistenceTimerRef.current !== null) {
      window.clearTimeout(stagedPersistenceTimerRef.current)
      stagedPersistenceTimerRef.current = null
    }

    void persistStagedControlsRef.current()
  }

  useEffect(() => {
    if (!initializedPersistenceRef.current) {
      initializedPersistenceRef.current = true
      return
    }

    scheduleStagedControlsPersist()
  }, [
    activePowerProfile,
    activeFanProfile,
    customProcessorState.min,
    customProcessorState.max,
    customPowerBase,
    gpuOverclock.coreClock,
    gpuOverclock.memoryClock,
    gpuOverclock.powerLimit,
    gpuOverclock.tempLimit,
    gpuOverclock.voltageOffset,
    activeOcSlot,
    ocApplyState,
    ocTuningLocked,
    customCurves,
    fanPresets,
    powerPresets,
    quickPresets,
    fanSyncLockEnabled,
    smartChargingEnabled,
    processorStateControlEnabled,
    nvidiaTelemetryEnabled,
    autoRefreshRateOnBatteryEnabled,
    autoRefreshRateRestoreHz,
    usbPowerEnabled,
    blueLightFilterEnabled,
    selectedBootArt,
    customBootFilename,
    updateChannel,
    checkForUpdatesOnLaunch,
  ])

  useEffect(() => {
    function flushOnHideOrExit() {
      if (document.visibilityState === 'hidden') {
        flushStagedControlsPersist()
      }
    }

    document.addEventListener('visibilitychange', flushOnHideOrExit)
    window.addEventListener('pagehide', flushStagedControlsPersist)
    window.addEventListener('beforeunload', flushStagedControlsPersist)

    return () => {
      document.removeEventListener('visibilitychange', flushOnHideOrExit)
      window.removeEventListener('pagehide', flushStagedControlsPersist)
      window.removeEventListener('beforeunload', flushStagedControlsPersist)
    }
  }, [])

  useEffect(() => {
    if (!isDesktopRuntime()) {
      return
    }

    let mounted = true
    const unlisteners: Array<() => void> = []

    async function wireTrayEvents() {
      unlisteners.push(
        await listen<TrayControlEventPayload>('tray-control-updated', (event) => {
          if (!mounted) {
            return
          }

          applyBackendControlSnapshot(
            event.payload.controls,
            setActivePowerProfile,
            setCustomProcessorState,
            setCustomPowerBase,
            setGpuOverclock,
            setCustomOcSlot,
            setActiveOcSlot,
            setOcApplyState,
            setOcTuningLocked,
            setActiveFanProfile,
            commitCustomCurves,
            setFanSyncLockEnabled,
            commitSmartChargingEnabled,
            commitProcessorStateControlEnabled,
            commitNvidiaTelemetryEnabled,
            setUsbPowerEnabled,
            commitBlueLightFilterEnabled,
            setSelectedBootArt,
            setCustomBootFilename,
            setUpdateChannel,
            setCheckForUpdatesOnLaunch,
            commitAutoRefreshRateSettings,
            (controls) => syncPresetCollectionsRef.current(controls),
          )
          setStatusMessage(`${event.payload.action} applied from tray. ${event.payload.detail}`)
          void refreshCachedLiveControlsAfterApplyRef.current().catch((error) => {
            pushDebugEventRef.current(`tray refresh failed: ${describeError(error)}`)
          })
        }),
      )

      unlisteners.push(
        await listen<TrayControlErrorPayload>('tray-control-error', (event) => {
          if (!mounted) {
            return
          }

          setStatusMessage(`${event.payload.action} failed from tray: ${event.payload.detail}`)
          pushDebugEventRef.current(`tray action failed: ${event.payload.detail}`)
        }),
      )

      unlisteners.push(
        await listen('tray-window-will-hide', () => {
          flushStagedControlsPersist()
        }),
      )
    }

    void wireTrayEvents().catch((error) => {
      pushDebugEventRef.current(`tray event listener setup failed: ${describeError(error)}`)
    })

    return () => {
      mounted = false
      unlisteners.splice(0).forEach((unlisten) => unlisten())
    }
  }, [])

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      const action = parseControlShortcut(event)
      if (!action) {
        return
      }

      event.preventDefault()

      switch (action.type) {
        case 'tab':
          setActiveTab(action.tab)
          pushDebugEventRef.current(`keyboard shortcut opened ${action.tab} tab`)
          break
        case 'power':
          void handlePowerProfileRef.current(action.profile)
          pushDebugEventRef.current(`keyboard shortcut power ${action.profile}`)
          break
        case 'fan':
          void handleFanProfileRef.current(action.profile)
          pushDebugEventRef.current(`keyboard shortcut fan ${action.profile}`)
          break
        case 'copy-diagnostics':
          void copyDiagnosticsToClipboardRef.current()
          break
        case 'export-diagnostics':
          exportDiagnosticsFileRef.current()
          break
        case 'export-settings':
          exportSettingsBackupRef.current()
          break
        case 'restore-balanced-auto':
          void handlePowerProfileRef.current('balanced')
          void handleFanProfileRef.current('auto')
          showToast(
            'info',
            'Restoring Balanced / Auto',
            'Keyboard shortcut requested Balanced power and Auto fans.',
          )
          break
        case 'dismiss-toast':
          dismissNewestToastRef.current()
          break
        case 'help':
          setShortcutsHelpOpen((open) => !open)
          break
        default:
          break
      }
    }

    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
    // eslint-disable-next-line react-hooks/exhaustive-deps -- global shortcut binding; showToast is feedback-only
  }, [])

  useEffect(() => {
    const resetStageFit = (
      setScale: (value: number) => void,
      setScaledHeight: (value: number | null) => void,
    ) => {
      setScale(1)
      setScaledHeight(null)
    }

    if (activeTab !== 'home') {
      resetStageFit(setHomeScale, setHomeScaledHeight)
    }
    if (activeTab !== 'fans') {
      resetStageFit(setFansScale, setFansScaledHeight)
    }
    if (activeTab !== 'power') {
      resetStageFit(setPowerScale, setPowerScaledHeight)
    }

    if (!isDevBuild() && activeTab !== 'debug') {
      return
    }

    const activeStage =
      activeTab === 'home'
        ? {
            id: 'home' as const,
            ref: homeStageRef,
            setScale: setHomeScale,
            setScaledHeight: setHomeScaledHeight,
          }
        : activeTab === 'fans'
          ? {
              id: 'fans' as const,
              ref: fansStageRef,
              setScale: setFansScale,
              setScaledHeight: setFansScaledHeight,
            }
          : activeTab === 'power'
            ? {
                id: 'power' as const,
                ref: powerStageRef,
                setScale: setPowerScale,
                setScaledHeight: setPowerScaledHeight,
              }
            : null

    if (!activeStage) {
      return
    }

    let frameId = 0
    let lastScale = -1
    let lastScaledHeight: number | null = null

    const measureStageFit = () => {
      const dashboard = dashboardRef.current
      const topbar = topbarRef.current
      const stage = activeStage.ref.current

      if (!dashboard || !topbar || !stage) {
        return
      }

      const dashboardStyle = window.getComputedStyle(dashboard)
      const topbarStyle = window.getComputedStyle(topbar)
      const topPadding = Number.parseFloat(dashboardStyle.paddingTop || '0') || 0
      const bottomPadding = Number.parseFloat(dashboardStyle.paddingBottom || '0') || 0
      const topbarMarginBottom = Number.parseFloat(topbarStyle.marginBottom || '0') || 0
      const reservedHeight =
        topPadding +
        bottomPadding +
        topbar.getBoundingClientRect().height +
        topbarMarginBottom +
        6
      const availableHeight = Math.max(0, window.innerHeight - reservedHeight)
      const naturalHeight = stage.scrollHeight
      const naturalWidth = stage.scrollWidth
      const availableWidth = dashboard.clientWidth

      if (naturalHeight <= 0 || naturalWidth <= 0) {
        if (lastScale !== 1) {
          activeStage.setScale(1)
          lastScale = 1
        }
        if (lastScaledHeight != null) {
          activeStage.setScaledHeight(null)
          lastScaledHeight = null
        }
        return
      }

      const nextScale = Math.min(1, availableWidth / naturalWidth)
      const clampedScale = Number.isFinite(nextScale) ? Math.max(0.92, nextScale) : 1
      const nextScaledHeight = naturalHeight * clampedScale
      const signature = [
        clampedScale.toFixed(3),
        Math.round(nextScaledHeight),
        Math.round(naturalWidth),
        Math.round(naturalHeight),
        Math.round(availableWidth),
        Math.round(availableHeight),
        window.innerWidth,
        window.innerHeight,
      ].join('|')

      if (stageFitSignatureRef.current[activeStage.id] !== signature) {
        const snapshot: StageFitSnapshot = {
          tab: activeStage.id,
          scale: clampedScale,
          scaledHeight: nextScaledHeight,
          naturalWidth,
          naturalHeight,
          availableWidth,
          availableHeight,
          windowWidth: window.innerWidth,
          windowHeight: window.innerHeight,
          updatedAt: formatDebugClock(new Date()),
        }

        stageFitSignatureRef.current[activeStage.id] = signature
        setStageFitSnapshots((current) => ({
          ...current,
          [activeStage.id]: snapshot,
        }))
        pushDebugEventRef.current(
          `stage fit ${activeStage.id}: scale ${snapshot.scale.toFixed(3)}, height ${Math.round(
            snapshot.scaledHeight,
          )}px, natural ${Math.round(snapshot.naturalWidth)}x${Math.round(
            snapshot.naturalHeight,
          )}, available ${Math.round(snapshot.availableWidth)}x${Math.round(
            snapshot.availableHeight,
          )}`,
        )
        queuePerformanceEventRef.current('stage-fit', `stage fit ${activeStage.id}`, {
          tab: snapshot.tab,
          scale: snapshot.scale,
          scaledHeight: snapshot.scaledHeight,
          naturalWidth: snapshot.naturalWidth,
          naturalHeight: snapshot.naturalHeight,
          availableWidth: snapshot.availableWidth,
          availableHeight: snapshot.availableHeight,
          windowWidth: snapshot.windowWidth,
          windowHeight: snapshot.windowHeight,
        })
      }

      if (Math.abs(lastScale - clampedScale) > 0.001) {
        activeStage.setScale(clampedScale)
        lastScale = clampedScale
      }

      if (lastScaledHeight == null || Math.abs(lastScaledHeight - nextScaledHeight) > 0.5) {
        activeStage.setScaledHeight(nextScaledHeight)
        lastScaledHeight = nextScaledHeight
      }
    }

    let lastResizeEventLoggedAt = 0

    const scheduleMeasure = () => {
      cancelAnimationFrame(frameId)
      frameId = requestAnimationFrame(measureStageFit)
    }

    const scheduleMeasureFromWindowResize = () => {
      const now = performance.now()
      if (now - lastResizeEventLoggedAt >= 500) {
        lastResizeEventLoggedAt = now
        queuePerformanceEventRef.current('window-resize', `window ${window.innerWidth}x${window.innerHeight}`, {
          windowWidth: window.innerWidth,
          windowHeight: window.innerHeight,
          tab: activeStage.id,
        })
      }
      scheduleMeasure()
    }

    const resizeObserver =
      typeof ResizeObserver !== 'undefined'
        ? new ResizeObserver(scheduleMeasure)
        : null

    scheduleMeasure()
    window.addEventListener('resize', scheduleMeasureFromWindowResize)

    if (resizeObserver) {
      if (dashboardRef.current) {
        resizeObserver.observe(dashboardRef.current)
      }
      if (topbarRef.current) {
        resizeObserver.observe(topbarRef.current)
      }
      if (activeStage.ref.current) {
        resizeObserver.observe(activeStage.ref.current)
      }
    }

    return () => {
      window.removeEventListener('resize', scheduleMeasureFromWindowResize)
      resizeObserver?.disconnect()
      cancelAnimationFrame(frameId)
    }
  }, [activeTab])

  function schedulePerformanceLogFlush() {
    if (performanceLogFlushTimerRef.current !== null) {
      return
    }

    performanceLogFlushTimerRef.current = window.setTimeout(() => {
      performanceLogFlushTimerRef.current = null
      void flushPerformanceLog()
    }, PERFORMANCE_LOG_FLUSH_DELAY_MS)
  }

  function diagnosticsEnabled() {
    return isDevBuild() || activeTabRef.current === 'debug'
  }

  function queuePerformanceEvent(
    eventType: string,
    detail: string,
    payload: Record<string, unknown> = {},
  ) {
    if (!isDesktopRuntime() || !diagnosticsEnabled()) {
      return
    }

    performanceLogQueueRef.current.push({
      sessionId: performanceLogSessionIdRef.current,
      eventType,
      occurredAtUnixMs: Date.now(),
      activeTab: activeTabRef.current,
      detail,
      payload: {
        ...payload,
        visibility: document.visibilityState,
      },
    })

    performanceLogEventCountRef.current += 1

    if (performanceLogQueueRef.current.length > PERFORMANCE_LOG_MAX_QUEUE) {
      performanceLogQueueRef.current.splice(
        0,
        performanceLogQueueRef.current.length - PERFORMANCE_LOG_MAX_QUEUE,
      )
    }

    if (performanceLogQueueRef.current.length >= PERFORMANCE_LOG_BATCH_SIZE) {
      void flushPerformanceLog()
    } else {
      schedulePerformanceLogFlush()
    }
  }

  async function flushPerformanceLog() {
    if (!isDesktopRuntime() || performanceLogFlushInFlightRef.current) {
      return
    }

    if (performanceLogFlushTimerRef.current !== null) {
      window.clearTimeout(performanceLogFlushTimerRef.current)
      performanceLogFlushTimerRef.current = null
    }

    const events = performanceLogQueueRef.current.splice(0)
    if (events.length === 0) {
      setPerformanceLogState((current) => ({
        ...current,
        pendingCount: 0,
        eventCount: performanceLogEventCountRef.current,
      }))
      return
    }

    performanceLogFlushInFlightRef.current = true

    try {
      const path = await appendPerformanceLog(events)
      performanceLogPathRef.current = path
      performanceLogLastErrorRef.current = null
      setPerformanceLogState({
        path,
        lastFlushAt: formatDebugClock(new Date()),
        pendingCount: performanceLogQueueRef.current.length,
        eventCount: performanceLogEventCountRef.current,
        lastError: null,
      })
    } catch (error) {
      const message = describeError(error)
      performanceLogLastErrorRef.current = message
      setPerformanceLogState({
        path: performanceLogPathRef.current,
        lastFlushAt: formatDebugClock(new Date()),
        pendingCount: performanceLogQueueRef.current.length,
        eventCount: performanceLogEventCountRef.current,
        lastError: message,
      })
      pushDebugEvent(`performance log write failed: ${message}`)
    } finally {
      performanceLogFlushInFlightRef.current = false
      if (performanceLogQueueRef.current.length > 0) {
        schedulePerformanceLogFlush()
      }
    }
  }
  queuePerformanceEventRef.current = queuePerformanceEvent
  flushPerformanceLogRef.current = flushPerformanceLog

  function pushDebugEvent(message: string) {
    const entry = `${formatDebugClock(new Date())} ${message}`

    if (diagnosticsEnabled()) {
      console.debug(`[AeroForge debug] ${entry}`)
    }

    const next = [entry, ...debugEventsRef.current].slice(0, 8)
    debugEventsRef.current = next

    if (activeTabRef.current === 'debug') {
      setDebugEvents(next)
    }
  }
  pushDebugEventRef.current = pushDebugEvent

  function pushTransportDebugEvent(message: string) {
    if (lastTransportDebugRef.current === message) {
      return
    }

    lastTransportDebugRef.current = message
    pushDebugEvent(message)
  }

  function pushPollHeartbeat(message: string) {
    const now = Date.now()
    if (now - lastPollHeartbeatRef.current < 5000) {
      return
    }

    lastPollHeartbeatRef.current = now
    pushDebugEvent(message)
  }
  pushTransportDebugEventRef.current = pushTransportDebugEvent
  pushPollHeartbeatEventRef.current = pushPollHeartbeat

  function beginControlApply() {
    controlApplyInFlightRef.current += 1
  }

  function endControlApply() {
    controlApplyInFlightRef.current = Math.max(0, controlApplyInFlightRef.current - 1)
  }

  function dismissToast(id: string) {
    const timer = toastTimersRef.current[id]
    if (timer !== undefined) {
      window.clearTimeout(timer)
      delete toastTimersRef.current[id]
    }
    setToasts((current) => current.filter((toast) => toast.id !== id))
  }

  function dismissNewestToast() {
    setToasts((current) => {
      if (current.length === 0) {
        return current
      }
      const newest = current[current.length - 1]
      const timer = toastTimersRef.current[newest.id]
      if (timer !== undefined) {
        window.clearTimeout(timer)
        delete toastTimersRef.current[newest.id]
      }
      return current.slice(0, -1)
    })
  }
  dismissNewestToastRef.current = dismissNewestToast

  function showToast(
    tone: AppToast['tone'],
    title: string,
    detail: string,
    durationMs = 4200,
  ) {
    const id = `toast-${Date.now().toString(36)}-${toastSequenceRef.current++}`
    setToasts((current) => [...current.slice(-3), { id, tone, title, detail }])
    if (durationMs > 0) {
      toastTimersRef.current[id] = window.setTimeout(() => dismissToast(id), durationMs)
    }
    return id
  }

  function updateToast(
    id: string,
    tone: AppToast['tone'],
    title: string,
    detail: string,
    durationMs = 4200,
  ) {
    const timer = toastTimersRef.current[id]
    if (timer !== undefined) {
      window.clearTimeout(timer)
      delete toastTimersRef.current[id]
    }
    setToasts((current) =>
      current.map((toast) => (toast.id === id ? { ...toast, tone, title, detail } : toast)),
    )
    if (durationMs > 0) {
      toastTimersRef.current[id] = window.setTimeout(() => dismissToast(id), durationMs)
    }
  }

  async function refreshHardwareReadiness(force = false) {
    const now = Date.now()
    if (!force && now - hardwareReadinessRefreshRef.current < HARDWARE_READINESS_REFRESH_MS) {
      return
    }

    hardwareReadinessRefreshRef.current = now
    try {
      const snapshot = await getHardwareReadinessSnapshot()
      setHardwareReadiness(snapshot)
      maybeRequestBackgroundServiceHealRef.current(snapshot)
      if (!snapshot.ready) {
        pushTransportDebugEventRef.current(`hardware readiness: ${snapshot.detail}`)
      }
    } catch (error) {
      pushTransportDebugEventRef.current(
        `hardware readiness probe failed: ${describeError(error)}`,
      )
    }
  }
  refreshHardwareReadinessRef.current = refreshHardwareReadiness

  async function refreshCachedLiveControlsAfterApply() {
    const refreshStartedMs = performance.now()

    try {
      const snapshot = await getBackendPollSnapshot()
      setServiceConnected(snapshot.service.connected)
      setTelemetrySourceLabel(describeTelemetrySource(snapshot.service.connected, snapshot.telemetry))
      setLastBackendError(snapshot.service.connected ? null : snapshot.service.detail)
      updateSerializedState(telemetrySnapshotRef, snapshot.telemetry, setLiveTelemetry)
      updateSerializedState(liveControlSnapshotStateRef, snapshot.liveControls, (next) => {
        liveControlSnapshotRef.current = next
        setLiveControlSnapshot(next)
      })
      if (activeTabRef.current === 'debug') {
        updateSerializedState(debugServiceStatusRef, snapshot.service, setServiceStatus)
        setLastBackendPollAt(formatDebugClock(new Date()))
      }

      return {
        liveControls: snapshot.liveControls,
        refreshMs: performance.now() - refreshStartedMs,
      }
    } catch (error) {
      pushTransportDebugEventRef.current(`post-apply cached refresh failed: ${describeError(error)}`)
      return {
        liveControls: null,
        refreshMs: performance.now() - refreshStartedMs,
      }
    }
  }
  refreshCachedLiveControlsAfterApplyRef.current = refreshCachedLiveControlsAfterApply

  useEffect(() => {
    queuePerformanceEventRef.current('performance-log-started', 'performance logging started', {
      windowWidth: window.innerWidth,
      windowHeight: window.innerHeight,
      userAgent: navigator.userAgent,
    })

    return () => {
      if (performanceLogFlushTimerRef.current !== null) {
        window.clearTimeout(performanceLogFlushTimerRef.current)
        performanceLogFlushTimerRef.current = null
      }
      void flushPerformanceLogRef.current()
    }
  }, [])

  async function syncAutoRefreshRateState(
    enabled: boolean,
    onBattery: boolean,
    announce: boolean,
  ) {
    const result = await applyAutoRefreshRate(enabled, onBattery)
    commitAutoRefreshRateSettings(
      result.controls.personalSettings.autoRefreshRateOnBatteryEnabled,
      result.controls.personalSettings.autoRefreshRateRestoreHz,
    )
    autoRefreshRateSyncKeyRef.current = `${result.enabled}:${result.onBattery}:${
      result.restoreHz ?? 'none'
    }`

    if (announce || result.appliedHz !== null) {
      setStatusMessage(result.detail)
    }
    if (result.appliedHz !== null) {
      pushDebugEvent(result.detail)
    }

    return result
  }
  syncAutoRefreshRateStateRef.current = syncAutoRefreshRateState

  useEffect(() => {
    if (displayedAcPluggedIn === null) {
      return
    }
    if (!autoRefreshRateOnBatteryEnabled && autoRefreshRateRestoreHz === null) {
      return
    }

    const onBattery = displayedAcPluggedIn === false
    const syncKey = `${autoRefreshRateOnBatteryEnabled}:${onBattery}:${
      autoRefreshRateRestoreHz ?? 'none'
    }`

    if (autoRefreshRateSyncKeyRef.current === syncKey) {
      return
    }

    autoRefreshRateSyncKeyRef.current = syncKey
    void syncAutoRefreshRateStateRef.current(autoRefreshRateOnBatteryEnabled, onBattery, false).catch(
      (error) => {
        autoRefreshRateSyncKeyRef.current = null
        pushDebugEventRef.current(`auto refresh-rate sync failed: ${describeError(error)}`)
      },
    )
  }, [autoRefreshRateOnBatteryEnabled, autoRefreshRateRestoreHz, displayedAcPluggedIn])

  useEffect(() => {
    if (displayedAcPluggedIn === true) {
      runtimeEstimateSessionRef.current = false
      setRuntimeEstimateCountdownSec(0)
    } else if (displayedAcPluggedIn === false && displayedBatteryLifeRemainingSec != null) {
      runtimeEstimateSessionRef.current = true
      setRuntimeEstimateCountdownSec(0)
    } else if (
      displayedAcPluggedIn === false &&
      displayedBatteryLifeRemainingSec == null &&
      !runtimeEstimateSessionRef.current
    ) {
      runtimeEstimateSessionRef.current = true
      setRuntimeEstimateCountdownSec(RUNTIME_ESTIMATE_COUNTDOWN_SEC)
    }
  }, [displayedAcPluggedIn, displayedBatteryLifeRemainingSec])

  useEffect(() => {
    if (runtimeEstimateCountdownSec <= 0) {
      return
    }

    const timer = window.setInterval(() => {
      setRuntimeEstimateCountdownSec((current) => {
        if (current <= 1) {
          window.clearInterval(timer)
          return 0
        }

        return current - 1
      })
    }, 1000)

    return () => {
      window.clearInterval(timer)
    }
  }, [runtimeEstimateCountdownSec])

  useEffect(() => {
    if (!draggingPoint) {
      return
    }

    const drag = draggingPoint

    function onPointerMove(event: PointerEvent) {
      const chart = chartRefs.current[drag.target]
      if (!chart) {
        return
      }

      const rect = chart.getBoundingClientRect()
      const usableWidth = chartWidth - chartPadding * 2
      const usableHeight = chartHeight - chartPadding * 2

      const viewBoxX = ((event.clientX - rect.left) / rect.width) * chartWidth
      const viewBoxY = ((event.clientY - rect.top) / rect.height) * chartHeight
      const x = clamp(viewBoxX, chartPadding, chartWidth - chartPadding)
      const y = clamp(viewBoxY, chartPadding, chartHeight - chartPadding)

      const temp = Math.round(
        tempMin + ((x - chartPadding) / usableWidth) * (tempMax - tempMin),
      )
      const speed = Math.round(
        speedMax - ((y - chartPadding) / usableHeight) * (speedMax - speedMin),
      )

      commitCustomCurves((current) => {
        const nextCurve = current[drag.target].map((point, index, items) => {
          if (index !== drag.index) {
            return point
          }

          const minTemp = index === 0 ? tempMin : items[index - 1].temp + 2
          const maxTemp = index === items.length - 1 ? tempMax : items[index + 1].temp - 2
          const minSpeed = index === 0 ? speedMin : items[index - 1].speed
          const maxSpeed = index === items.length - 1 ? speedMax : items[index + 1].speed

          return {
            temp: clamp(temp, minTemp, maxTemp),
            speed: clamp(speed, minSpeed, maxSpeed),
          }
        })

        const nextCurves = {
          ...current,
          [drag.target]: nextCurve,
        }

        return fanSyncLockEnabled ? mirrorCurveSetFromTarget(nextCurves, drag.target) : nextCurves
      })
    }

    function onPointerUp() {
      setDraggingPoint(null)
      void finalizeCustomCurveEditRef.current(customCurvesRef.current)
    }

    window.addEventListener('pointermove', onPointerMove)
    window.addEventListener('pointerup', onPointerUp)

    return () => {
      window.removeEventListener('pointermove', onPointerMove)
      window.removeEventListener('pointerup', onPointerUp)
    }
  }, [draggingPoint, fanSyncLockEnabled])

  useEffect(() => {
    if (!customBootPreview) {
      return
    }

    return () => {
      URL.revokeObjectURL(customBootPreview)
    }
  }, [customBootPreview])

  useEffect(() => {
    if (!isDevBuild() && activeTab !== 'debug') {
      return
    }

    let cancelled = false
    let rafId = 0
    let previousTimestamp = 0
    let sampleStartedAt = 0
    let frameCount = 0
    let totalDeltaMs = 0
    let maxDeltaMs = 0
    let longFrameCount = 0
    let lastLoggedWindowAt = 0

    function step(timestamp: number) {
      if (cancelled) {
        return
      }

      if (previousTimestamp === 0) {
        previousTimestamp = timestamp
        sampleStartedAt = timestamp
        rafId = window.requestAnimationFrame(step)
        return
      }

      const deltaMs = timestamp - previousTimestamp
      previousTimestamp = timestamp
      frameCount += 1
      totalDeltaMs += deltaMs
      maxDeltaMs = Math.max(maxDeltaMs, deltaMs)

      if (deltaMs >= PERFORMANCE_LOG_LONG_FRAME_MS) {
        longFrameCount += 1
      }

      const sampleWindowMs = timestamp - sampleStartedAt

      if (sampleWindowMs >= 1000) {
        const averageMs = frameCount > 0 ? totalDeltaMs / frameCount : 0
        const fps = averageMs > 0 ? 1000 / averageMs : 0
        const updatedAt = formatDebugClock(new Date())

        setFrameStats({
          averageMs,
          maxMs: maxDeltaMs,
          fps,
          longFrameCount,
          sampleWindowMs,
          updatedAt,
        })

        const activeFrameTab = activeTabRef.current
        const activeFit =
          activeFrameTab === 'home' || activeFrameTab === 'power' || activeFrameTab === 'fans'
            ? stageFitSnapshotsRef.current[activeFrameTab]
            : null
        queuePerformanceEventRef.current('frame-sample', `frame sample ${activeFrameTab}`, {
          averageMs,
          maxMs: maxDeltaMs,
          fps,
          longFrameCount,
          sampleWindowMs,
          windowWidth: window.innerWidth,
          windowHeight: window.innerHeight,
          activeTab: activeFrameTab,
          stageFit: activeFit,
        })

        if (
          (maxDeltaMs >= 50 || longFrameCount >= 3 || averageMs >= 20) &&
          timestamp - lastLoggedWindowAt >= 2000
        ) {
          pushDebugEventRef.current(
            `frame sample: avg ${averageMs.toFixed(1)} ms / max ${maxDeltaMs.toFixed(1)} ms / ${Math.round(
              fps,
            )} fps / ${longFrameCount} long`,
          )
          lastLoggedWindowAt = timestamp
        }

        sampleStartedAt = timestamp
        frameCount = 0
        totalDeltaMs = 0
        maxDeltaMs = 0
        longFrameCount = 0
      }

      rafId = window.requestAnimationFrame(step)
    }

    rafId = window.requestAnimationFrame(step)

    return () => {
      cancelled = true
      window.cancelAnimationFrame(rafId)
    }
  }, [activeTab])

  useEffect(() => {
    const tauriInternals = (
      window as Window & { __TAURI_INTERNALS__?: unknown }
    ).__TAURI_INTERNALS__

    if (!tauriInternals) {
      return
    }

    let cancelled = false

    async function detectShell() {
      const bootstrapStartedAt = performance.now()
      let backendBootstrapMs = 0
      let persistenceMs = 0
      let updaterMs = 0
      let liveControlsMs = 0

      try {
        const backendBootstrapStartedAt = performance.now()
        const bootstrap = await getBackendBootstrap()
        backendBootstrapMs = performance.now() - backendBootstrapStartedAt
        const persistence = bootstrap.persistence
        const updater = bootstrap.updateStatus
        persistenceMs = 0
        updaterMs = 0
        const runtime = bootstrap.shell
        const service = bootstrap.service
        const readiness = bootstrap.hardwareReadiness
        const telemetry = bootstrap.telemetry
        const liveControls = bootstrap.liveControls
        liveControlsMs = 0

        if (!cancelled) {
          setBackendCapabilities(bootstrap.capabilities)
          controlsBootstrappedRef.current = true
          applyBackendControlSnapshot(
            mergeControlsWithLiveSnapshot(bootstrap.controls, liveControls),
            setActivePowerProfile,
            setCustomProcessorState,
            setCustomPowerBase,
            setGpuOverclock,
            setCustomOcSlot,
            setActiveOcSlot,
            setOcApplyState,
            setOcTuningLocked,
            setActiveFanProfile,
            commitCustomCurves,
            setFanSyncLockEnabled,
            commitSmartChargingEnabled,
            commitProcessorStateControlEnabled,
            commitNvidiaTelemetryEnabled,
            setUsbPowerEnabled,
            commitBlueLightFilterEnabled,
            setSelectedBootArt,
            setCustomBootFilename,
            setUpdateChannel,
            setCheckForUpdatesOnLaunch,
            commitAutoRefreshRateSettings,
            (controls) => syncPresetCollectionsRef.current(controls),
          )
          setBackendVersion(runtime.backendVersion)
          setUpdateStatus(updater)
          setHardwareReadiness(readiness)
          maybeRequestBackgroundServiceHealRef.current(readiness)
          hardwareReadinessRefreshRef.current = Date.now()
          setShellStatus(`${runtime.shell} v${runtime.backendVersion}`)
          setServiceConnected(service.connected)
          setTelemetrySourceLabel(describeTelemetrySource(service.connected, telemetry))
          setLastBackendError(service.connected ? null : service.detail)
          updateSerializedState(telemetrySnapshotRef, telemetry, setLiveTelemetry)
          updateSerializedState(
            liveControlSnapshotStateRef,
            liveControls,
            (next) => {
              liveControlSnapshotRef.current = next
              setLiveControlSnapshot(next)
            },
          )
          if (activeTabRef.current === 'debug') {
            updateSerializedState(debugServiceStatusRef, service, setServiceStatus)
            setLastBackendPollAt(formatDebugClock(new Date()))
          }
          setStatusMessage(
            `Desktop backend ${runtime.backendVersion} loaded. State ${
              persistence.initializedFromDisk ? 'restored from disk' : 'started from defaults'
            }. Service ${
              service.connected
                ? `connected over named pipe with ${service.workerCount} workers online`
                : hasUsableTelemetry(telemetry)
                  ? 'not connected, showing cached service telemetry'
                  : 'not connected, no cached service telemetry available'
            }.`,
          )
        }

        pushTransportDebugEventRef.current(
          service.connected
            ? `bootstrap connected: ${service.detail}`
            : `bootstrap fallback: ${service.detail}`,
        )
        queuePerformanceEventRef.current(
          'backend-bootstrap',
          readiness.ready
            ? service.connected
              ? 'bootstrap connected'
              : 'bootstrap fallback'
            : 'bootstrap hardware blocked',
          {
            totalMs: performance.now() - bootstrapStartedAt,
            backendBootstrapMs,
            persistenceMs,
            updaterMs,
            liveControlsMs,
            serviceConnected: service.connected,
            workerCount: service.workerCount,
            hardwareReady: readiness.ready,
            hardwareDetail: readiness.detail,
            telemetryUsable: hasUsableTelemetry(telemetry),
          },
        )
      } catch (error) {
        const message = describeError(error)

        if (!cancelled) {
          setBackendCapabilities(null)
          setServiceConnected(false)
          setTelemetrySourceLabel('No telemetry')
          setLastBackendError(message)
          updateSerializedState(debugServiceStatusRef, null, setServiceStatus)
          updateSerializedState(liveControlSnapshotStateRef, null, (next) => {
            liveControlSnapshotRef.current = next
            setLiveControlSnapshot(next)
          })
          updateSerializedState(telemetrySnapshotRef, null, setLiveTelemetry)
          if (activeTabRef.current === 'debug') {
            setLastBackendPollAt(formatDebugClock(new Date()))
          }
          setStatusMessage(`Desktop bootstrap failed: ${message}`)
        }

        pushTransportDebugEventRef.current(`bootstrap error: ${message}`)
        queuePerformanceEventRef.current('backend-bootstrap-error', message, {
          totalMs: performance.now() - bootstrapStartedAt,
        })
      }
    }

    void detectShell()

    async function pollBackend() {
      if (backendPollInFlightRef.current || controlApplyInFlightRef.current > 0) {
        return
      }

      backendPollInFlightRef.current = true
      const pollStartedAt = new Date()
      const pollStartedMs = performance.now()
      let serviceMs = 0
      let telemetryMs = 0
      let liveControlsMs = 0
      let backendCommandMs = 0

      try {
        const backendCommandStartedMs = performance.now()
        const pollSnapshot = await getBackendPollSnapshot()
        backendCommandMs = performance.now() - backendCommandStartedMs
        const service = pollSnapshot.service
        const telemetry = pollSnapshot.telemetry
        const liveControls = pollSnapshot.liveControls
        serviceMs = pollSnapshot.timings.serviceMs
        telemetryMs = pollSnapshot.timings.telemetryMs
        liveControlsMs = pollSnapshot.timings.liveControlsMs

        if (!cancelled) {
          setServiceConnected(service.connected)
          setTelemetrySourceLabel(describeTelemetrySource(service.connected, telemetry))
          setLastBackendError(service.connected ? null : service.detail)
          updateSerializedState(telemetrySnapshotRef, telemetry, setLiveTelemetry)
          updateSerializedState(
            liveControlSnapshotStateRef,
            liveControls,
            (next) => {
              liveControlSnapshotRef.current = next
              setLiveControlSnapshot(next)
            },
          )
          if (activeTabRef.current === 'debug') {
            updateSerializedState(debugServiceStatusRef, service, setServiceStatus)
            setLastBackendPollAt(formatDebugClock(pollStartedAt))
          }
          void refreshHardwareReadinessRef.current()
        }

        if (service.connected) {
          pushPollHeartbeatEventRef.current(
            `poll connected: CPU ${presentPositive(
              telemetry?.cpuTempAverageC ?? telemetry?.cpuTempC ?? null,
            ) ?? '?'}C / GPU ${presentPositive(telemetry?.gpuTempC ?? null) ?? '?'}C`,
          )
        } else if (hasUsableTelemetry(telemetry)) {
          pushPollHeartbeatEventRef.current(
            `poll cached: CPU ${presentPositive(
              telemetry?.cpuTempAverageC ?? telemetry?.cpuTempC ?? null,
            ) ?? '?'}C / GPU ${presentPositive(telemetry?.gpuTempC ?? null) ?? '?'}C`,
          )
        } else {
          pushTransportDebugEventRef.current(`poll fallback: ${service.detail}`)
        }

        queuePerformanceEventRef.current(
          'backend-poll',
          service.connected ? 'poll connected' : hasUsableTelemetry(telemetry) ? 'poll cached' : 'poll fallback',
          {
            totalMs: performance.now() - pollStartedMs,
            serviceMs,
            telemetryMs,
            liveControlsMs,
            backendCommandMs,
            backendReadMs: pollSnapshot.timings.totalMs,
            serviceConnected: service.connected,
            workerCount: service.workerCount,
            telemetryUsable: hasUsableTelemetry(telemetry),
          },
        )
      } catch (error) {
        const message = describeError(error)

        if (!cancelled) {
          setServiceConnected(false)
          setTelemetrySourceLabel('No telemetry')
          setLastBackendError(message)
          updateSerializedState(debugServiceStatusRef, null, setServiceStatus)
          updateSerializedState(liveControlSnapshotStateRef, null, (next) => {
            liveControlSnapshotRef.current = next
            setLiveControlSnapshot(next)
          })
          updateSerializedState(telemetrySnapshotRef, null, setLiveTelemetry)
          if (activeTabRef.current === 'debug') {
            setLastBackendPollAt(formatDebugClock(pollStartedAt))
          }
        }

        pushTransportDebugEventRef.current(`poll error: ${message}`)
        queuePerformanceEventRef.current('backend-poll-error', message, {
          totalMs: performance.now() - pollStartedMs,
          serviceMs,
          telemetryMs,
          liveControlsMs,
          backendCommandMs,
        })
      } finally {
        backendPollInFlightRef.current = false
      }
    }

    let pollTimer = 0

    function currentPollInterval() {
      if (document.visibilityState === 'hidden') {
        return HIDDEN_BACKEND_POLL_INTERVAL_MS
      }

      return activeTabRef.current === 'personal' || activeTabRef.current === 'debug'
        ? BACKEND_SECONDARY_POLL_INTERVAL_MS
        : BACKEND_POLL_INTERVAL_MS
    }

    function scheduleNextPoll(delay = currentPollInterval()) {
      window.clearTimeout(pollTimer)
      pollTimer = window.setTimeout(() => {
        void pollBackend().finally(() => {
          if (!cancelled) {
            scheduleNextPoll()
          }
        })
      }, delay)
    }

    function pollNowThenReschedule() {
      window.clearTimeout(pollTimer)
      void pollBackend().finally(() => {
        if (!cancelled) {
          scheduleNextPoll()
        }
      })
    }

    function onVisibilityChange() {
      if (document.visibilityState === 'visible') {
        pollNowThenReschedule()
      }
    }

    document.addEventListener('visibilitychange', onVisibilityChange)
    scheduleNextPoll()

    return () => {
      cancelled = true
      backendPollInFlightRef.current = false
      document.removeEventListener('visibilitychange', onVisibilityChange)
      window.clearTimeout(pollTimer)
    }
  }, [])

  useEffect(() => {
    if (autoUpdateCheckTriggeredRef.current) {
      return
    }

    if (!checkForUpdatesOnLaunch || !isDesktopRuntime()) {
      return
    }

    autoUpdateCheckTriggeredRef.current = true
    void runUpdateCheckRef.current?.(false)
  }, [checkForUpdatesOnLaunch])

  function pulseControl(target: string) {
    setGlowTarget(target)
    if (pulseTimerRef.current !== null) {
      window.clearTimeout(pulseTimerRef.current)
    }
    pulseTimerRef.current = window.setTimeout(() => {
      pulseTimerRef.current = null
      setGlowTarget('')
    }, 1200)
  }

  function buildCurrentDiagnosticsBundle() {
    return buildDiagnosticsBundle({
      generatedAt: new Date().toISOString(),
      appVersion: backendVersion,
      shellStatus,
      statusMessage,
      serviceConnected,
      serviceStatus,
      hardwareReadiness,
      capabilities: backendCapabilities,
      activePowerProfile,
      activeFanProfile,
      processorState: customProcessorState,
      customPowerBase,
      processorStateControlEnabled,
      smartChargingEnabled,
      nvidiaTelemetryEnabled,
      blueLightFilterEnabled,
      telemetry: activeTelemetry,
      liveControls: liveControlSnapshot,
      lastBackendError,
      lastBackendPollAt,
      telemetrySourceLabel,
      recentEvents: debugEventsRef.current,
      frameStats: {
        averageMs: frameStats.averageMs,
        maxMs: frameStats.maxMs,
        fps: frameStats.fps,
        longFrameCount: frameStats.longFrameCount,
        updatedAt: frameStats.updatedAt,
      },
      performanceLogPath: performanceLogState.path,
    })
  }

  async function copyDiagnosticsToClipboard() {
    try {
      const bundle = buildCurrentDiagnosticsBundle()
      const text = formatDiagnosticsText(bundle)
      await copyTextToClipboard(text)
      pushDebugEvent('diagnostics copied to clipboard')
      showToast('success', 'Diagnostics copied', 'A text report is ready to paste into Discord or a ticket.')
    } catch (error) {
      const detail = describeError(error)
      pushDebugEvent(`diagnostics copy failed: ${detail}`)
      showToast('error', 'Could not copy diagnostics', detail)
    }
  }

  function exportDiagnosticsFile() {
    try {
      const bundle = buildCurrentDiagnosticsBundle()
      const stamp = new Date().toISOString().replace(/[:.]/g, '-').slice(0, 19)
      downloadTextFile(
        `AeroForge-Diagnostics-${stamp}.json`,
        `${JSON.stringify(bundle, null, 2)}\n`,
        'application/json',
      )
      pushDebugEvent('diagnostics JSON exported')
      showToast('success', 'Diagnostics exported', 'JSON diagnostic bundle saved.')
    } catch (error) {
      const detail = describeError(error)
      pushDebugEvent(`diagnostics export failed: ${detail}`)
      showToast('error', 'Could not export diagnostics', detail)
    }
  }
  exportDiagnosticsFileRef.current = exportDiagnosticsFile

  function clearDebugEvents() {
    const entry = `${formatDebugClock(new Date())} debug event log cleared`
    debugEventsRef.current = [entry]
    setDebugEvents([entry])
  }

  function exportPresetPack() {
    try {
      const pack = buildPresetPack({
        appVersion: backendVersion,
        fanPresets,
        powerPresets,
        quickPresets,
      })
      const stamp = new Date().toISOString().replace(/[:.]/g, '-').slice(0, 19)
      downloadTextFile(
        `AeroForge-Presets-${stamp}.json`,
        `${JSON.stringify(pack, null, 2)}\n`,
        'application/json',
      )
      showToast(
        'success',
        'Presets exported',
        `${fanPresets.length} fan, ${powerPresets.length} power, ${quickPresets.length} combo.`,
      )
    } catch (error) {
      showToast('error', 'Preset export failed', describeError(error))
    }
  }

  function triggerPresetImport() {
    presetImportInputRef.current?.click()
  }

  function exportSettingsBackup() {
    try {
      const controls = buildControlSnapshotForPersistence({
        activePowerProfile,
        activeFanProfile,
        customProcessorState,
        customPowerBase,
        gpuOverclock,
        ocProfileSlots,
        activeOcSlot,
        ocApplyState,
        ocTuningLocked,
        customCurves,
        fanPresets,
        powerPresets,
        quickPresets,
        fanSyncLockEnabled,
        smartChargingEnabled,
        processorStateControlEnabled,
        nvidiaTelemetryEnabled,
        autoRefreshRateOnBatteryEnabled,
        autoRefreshRateRestoreHz,
        usbPowerEnabled,
        blueLightFilterEnabled,
        selectedBootArt,
        customBootFilename,
        updateChannel,
        checkForUpdatesOnLaunch,
      })
      const payload = buildPortableSettings(controls, backendVersion)
      downloadTextFile(
        defaultSettingsExportFilename(backendVersion),
        serializePortableSettings(payload),
      )
      showToast(
        'success',
        'Settings exported',
        'Profiles, curves, presets, and personal settings were saved to a JSON backup.',
      )
      pushDebugEvent('portable settings exported')
    } catch (error) {
      showToast('error', 'Settings export failed', describeError(error))
    }
  }
  exportSettingsBackupRef.current = exportSettingsBackup

  function triggerSettingsImport() {
    settingsImportInputRef.current?.click()
  }

  async function handleSettingsImportFile(event: ChangeEvent<HTMLInputElement>) {
    const file = event.target.files?.[0]
    event.target.value = ''
    if (!file) {
      return
    }

    try {
      const raw = await file.text()
      const parsed = parsePortableSettings(raw)
      if (!parsed.ok) {
        showToast('error', 'Settings import failed', parsed.error)
        return
      }

      applyBackendControlSnapshot(
        parsed.payload.controls,
        setActivePowerProfile,
        setCustomProcessorState,
        setCustomPowerBase,
        setGpuOverclock,
        setCustomOcSlot,
        setActiveOcSlot,
        setOcApplyState,
        setOcTuningLocked,
        setActiveFanProfile,
        commitCustomCurves,
        setFanSyncLockEnabled,
        commitSmartChargingEnabled,
        commitProcessorStateControlEnabled,
        commitNvidiaTelemetryEnabled,
        setUsbPowerEnabled,
        commitBlueLightFilterEnabled,
        setSelectedBootArt,
        setCustomBootFilename,
        setUpdateChannel,
        setCheckForUpdatesOnLaunch,
        (enabled, restoreHz) => {
          setAutoRefreshRateOnBatteryEnabled(enabled)
          setAutoRefreshRateRestoreHz(restoreHz)
        },
        syncPresetCollectionsRef.current,
      )

      await persistStagedControlsRef.current({
        activePowerProfile: parsed.payload.controls.activePowerProfile,
        activeFanProfile: parsed.payload.controls.activeFanProfile,
        customProcessorState: {
          min: parsed.payload.controls.customProcessorState.minPercent,
          max: parsed.payload.controls.customProcessorState.maxPercent,
        },
        customPowerBase: parsed.payload.controls.customPowerBase,
        customCurves: fromBackendCurveSet(parsed.payload.controls.fanCurves),
        fanPresets: parsed.payload.controls.fanPresets,
        powerPresets: parsed.payload.controls.powerPresets,
        quickPresets: parsed.payload.controls.quickPresets,
        fanSyncLockEnabled: parsed.payload.controls.fanSyncLockEnabled,
        smartChargingEnabled: parsed.payload.controls.personalSettings.smartChargingEnabled,
        processorStateControlEnabled:
          parsed.payload.controls.personalSettings.processorStateControlEnabled,
        nvidiaTelemetryEnabled:
          parsed.payload.controls.personalSettings.nvidiaTelemetryEnabled,
        autoRefreshRateOnBatteryEnabled:
          parsed.payload.controls.personalSettings.autoRefreshRateOnBatteryEnabled,
        autoRefreshRateRestoreHz:
          parsed.payload.controls.personalSettings.autoRefreshRateRestoreHz,
        blueLightFilterEnabled:
          parsed.payload.controls.personalSettings.blueLightFilterEnabled,
        selectedBootArt: parsed.payload.controls.personalSettings.selectedBootArt,
        customBootFilename: parsed.payload.controls.personalSettings.customBootFilename,
        updateChannel: parsed.payload.controls.personalSettings.updateChannel,
        checkForUpdatesOnLaunch:
          parsed.payload.controls.personalSettings.checkForUpdatesOnLaunch,
      })

      showToast(
        'success',
        'Settings restored',
        parsed.payload.appVersion
          ? `Imported backup from AeroForge ${parsed.payload.appVersion}. Re-apply power/fan modes if you want them live on hardware.`
          : 'Imported backup. Re-apply power/fan modes if you want them live on hardware.',
      )
      pushDebugEvent('portable settings imported')
    } catch (error) {
      showToast('error', 'Settings import failed', describeError(error))
    }
  }

  async function handlePresetImportFile(event: ChangeEvent<HTMLInputElement>) {
    const file = event.target.files?.[0]
    event.target.value = ''
    if (!file) {
      return
    }

    try {
      const raw = await file.text()
      const { pack, warnings } = parsePresetPack(raw)
      const mode =
        fanPresets.length === 0 && powerPresets.length === 0 && quickPresets.length === 0
          ? 'replace'
          : window.confirm(
              'Merge imported presets with your existing list?\n\nOK = merge (recommended)\nCancel = replace all presets',
            )
            ? 'merge'
            : 'replace'
      const merged = mergePresetPack(
        {
          fanPresets: fanPresetsRef.current,
          powerPresets: powerPresetsRef.current,
          quickPresets: quickPresetsRef.current,
        },
        pack,
        mode,
      )
      commitFanPresets(merged.fanPresets)
      commitPowerPresets(merged.powerPresets)
      commitQuickPresets(merged.quickPresets)
      await persistStagedControlsRef.current({
        fanPresets: merged.fanPresets,
        powerPresets: merged.powerPresets,
        quickPresets: merged.quickPresets,
      })
      const warningSuffix = warnings.length > 0 ? ` ${warnings.join(' ')}` : ''
      showToast(
        'success',
        mode === 'merge' ? 'Presets merged' : 'Presets replaced',
        `Now ${merged.fanPresets.length} fan, ${merged.powerPresets.length} power, ${merged.quickPresets.length} combo.${warningSuffix}`,
      )
      pushDebugEvent(
        `preset pack imported (${mode}): ${pack.fanPresets.length} fan / ${pack.powerPresets.length} power / ${pack.quickPresets.length} combo`,
      )
    } catch (error) {
      const detail = describeError(error)
      pushDebugEvent(`preset import failed: ${detail}`)
      showToast('error', 'Preset import failed', detail)
    }
  }

  async function handlePowerProfile(profileId: PowerProfile['id']) {
    // A pending debounced Custom apply must not land after a different profile
    // was chosen: it would snap the UI back to Custom and race the hardware write.
    if (customPowerApplyTimerRef.current !== null) {
      window.clearTimeout(customPowerApplyTimerRef.current)
      customPowerApplyTimerRef.current = null
    }
    customPowerApplyRevisionRef.current += 1

    const nextProcessorState = getProcessorStateForPowerProfile(
      profileId,
      customProcessorStateRef.current,
    )
    const profileName = powerProfiles.find((profile) => profile.id === profileId)?.name ?? 'Power'
    const toastId = showToast('info', 'Applying power mode', `${profileName} requested.`, 0)

    if (profileId === 'custom') {
      customProcessorStateRef.current = nextProcessorState
      setCustomProcessorState(nextProcessorState)
    }

    const hasService = serviceConnectedRef.current
    const hasPowerHardware = hasService && (hardwareReadiness?.ready ?? true)
    const processorPolicyEnabled = processorStateControlEnabledRef.current

    setActivePowerProfile(profileId)
    setStatusMessage(
      hasPowerHardware
        ? processorPolicyEnabled
          ? `${profileName} profile apply requested.`
          : `${profileName} profile apply requested. CPU min/max writes are off.`
        : hardwareReadiness && !hardwareReadiness.ready
          ? `${profileName} profile saved locally. Hardware power writes are blocked: ${hardwareReadiness.detail}`
        : `${profileName} profile staged in the frontend preview.`,
    )
    pulseControl(profileId)
    queuePerformanceEvent('power-profile-select', profileId, {
      serviceConnected: hasService,
      hardwareReady: hasPowerHardware,
    })

    if (!hasPowerHardware) {
      await waitForNextPaint()
      await persistStagedControlsRef.current({
        activePowerProfile: profileId,
        ...(profileId === 'custom' ? { customProcessorState: nextProcessorState } : {}),
      })
      updateToast(
        toastId,
        'warning',
        'Power saved locally',
        hasPowerHardware
          ? `${profileName} is waiting for hardware availability.`
          : hardwareReadiness && !hardwareReadiness.ready
            ? hardwareReadiness.detail
            : `${profileName} is staged in preview mode.`,
      )
      return
    }

    if (powerProfileApplyInFlightRef.current) {
      queuedPowerProfileRef.current = profileId
      setStatusMessage(`${profileName} profile queued after the current hardware apply finishes.`)
      updateToast(toastId, 'info', 'Power queued', `${profileName} will apply after the current request.`)
      queuePerformanceEvent('power-profile-apply-queued', profileId)
      await waitForNextPaint()
      return
    }

    powerProfileApplyInFlightRef.current = true
    const applyStartedMs = performance.now()
    beginControlApply()
    queuePerformanceEvent('power-profile-apply-started', profileId, {
      minPercent: nextProcessorState.min,
      maxPercent: nextProcessorState.max,
      processorStateControlEnabled: processorPolicyEnabled,
    })

    try {
      await waitForNextPaint()
      const updatedControls = await applyPowerProfile(
        profileId,
        {
          minPercent: nextProcessorState.min,
          maxPercent: nextProcessorState.max,
        },
        profileId === 'custom' ? customPowerBaseRef.current : null,
        processorPolicyEnabled,
      )
      const refreshed = await refreshCachedLiveControlsAfterApply()
      const liveControls = refreshed.liveControls
      const layerAttention = summarizeLayerAttention(liveControls?.lastPowerLayerStatuses)
      const queuedProfile = queuedPowerProfileRef.current
      const resultSuperseded = queuedProfile !== null && queuedProfile !== profileId

      if (!resultSuperseded) {
        applyAuthoritativeSnapshot(mergeControlsWithLiveSnapshot(updatedControls, liveControls), 'power')
      }
      queuePerformanceEvent('power-profile-apply-finished', profileId, {
        totalMs: performance.now() - applyStartedMs,
        refreshMs: refreshed.refreshMs,
        superseded: resultSuperseded,
        queuedProfile,
      })

      if (!resultSuperseded) {
        if (layerAttention) {
          setStatusMessage(
            `${profileName} profile applied with layer attention: ${layerAttention}.`,
          )
          updateToast(
            toastId,
            'warning',
            'Power partially applied',
            `${profileName}: ${layerAttention}`,
          )
        } else if (!processorPolicyEnabled) {
          setStatusMessage(
            `${profileName} profile applied through the AeroForge service. CPU min/max writes were skipped by Settings.`,
          )
          updateToast(
            toastId,
            'success',
            'Power applied',
            `${profileName} applied. CPU min/max writes were skipped by Settings.`,
          )
        } else {
          setStatusMessage(
        `${profileName} profile applied through the AeroForge service${
          liveControls?.processorStateReadback
            ? ` and verified as AC ${liveControls.processorStateReadback.ac.minPercent}/${liveControls.processorStateReadback.ac.maxPercent} • DC ${liveControls.processorStateReadback.dc.minPercent}/${liveControls.processorStateReadback.dc.maxPercent}.`
            : '.'
        }`,
          )
          updateToast(toastId, 'success', 'Power applied', `${profileName} applied.`)
        }
      } else {
        updateToast(toastId, 'info', 'Power superseded', `${profileName} was replaced by a newer request.`)
      }
    } catch (error) {
      queuePerformanceEvent('power-profile-apply-failed', profileId, {
        totalMs: performance.now() - applyStartedMs,
        error: describeError(error),
      })
      setStatusMessage(
        `Power profile apply failed: ${error instanceof Error ? error.message : String(error)}`,
      )
      updateToast(toastId, 'error', 'Power failed', describeError(error))
    } finally {
      powerProfileApplyInFlightRef.current = false
      const queuedProfile = queuedPowerProfileRef.current
      queuedPowerProfileRef.current = null
      endControlApply()
      if (queuedProfile !== null && queuedProfile !== profileId) {
        queuePerformanceEvent('power-profile-apply-draining-queued', queuedProfile, {
          previousProfile: profileId,
        })
        void handlePowerProfile(queuedProfile)
      }
    }
  }

  function scheduleCustomPowerApply(
    nextProcessorState: { min: number; max: number },
    nextBase: CustomPowerBaseId,
    reason: string,
  ) {
    if (!powerHardwareApplyReady) {
      setStatusMessage(
        hardwareReadiness && !hardwareReadiness.ready
          ? `${reason}. Saved locally because hardware power writes are blocked: ${hardwareReadiness.detail}`
          : `${reason} in the preview.`,
      )
      return
    }

    if (customPowerApplyTimerRef.current !== null) {
      window.clearTimeout(customPowerApplyTimerRef.current)
    }

    const revision = customPowerApplyRevisionRef.current + 1
    customPowerApplyRevisionRef.current = revision
    setStatusMessage(
      processorStateControlEnabledRef.current
        ? `${reason}. Applying to Windows processor policy...`
        : `${reason}. Applying firmware mode with CPU min/max writes off...`,
    )

    customPowerApplyTimerRef.current = window.setTimeout(() => {
      customPowerApplyTimerRef.current = null
      void applyCustomPowerPolicy(revision, nextProcessorState, nextBase)
    }, 600)
  }

  async function applyCustomPowerPolicy(
    revision: number,
    nextProcessorState: { min: number; max: number },
    nextBase: CustomPowerBaseId,
  ) {
    if (!powerHardwareApplyReady) {
      return
    }

    const processorPolicyEnabled = processorStateControlEnabledRef.current

    try {
      const updatedControls = await applyPowerProfile(
        'custom',
        {
          minPercent: nextProcessorState.min,
          maxPercent: nextProcessorState.max,
        },
        nextBase,
        processorPolicyEnabled,
      )
      const liveControls = await getLiveControlSnapshot().catch(() => null)

      if (revision !== customPowerApplyRevisionRef.current) {
        return
      }

      applyAuthoritativeSnapshot(mergeControlsWithLiveSnapshot(updatedControls, liveControls), 'power')
      updateSerializedState(liveControlSnapshotStateRef, liveControls, (next) => {
        liveControlSnapshotRef.current = next
        setLiveControlSnapshot(next)
      })

      const layerAttention = summarizeLayerAttention(liveControls?.lastPowerLayerStatuses)
      setStatusMessage(
        layerAttention
          ? `Custom power policy applied with layer attention: ${layerAttention}.`
          : !processorPolicyEnabled
          ? 'Custom power policy applied. CPU min/max writes were skipped by Settings.'
          : liveControls?.processorStateReadback
          ? `Custom processor state applied and verified as AC ${liveControls.processorStateReadback.ac.minPercent}/${liveControls.processorStateReadback.ac.maxPercent} \u2022 DC ${liveControls.processorStateReadback.dc.minPercent}/${liveControls.processorStateReadback.dc.maxPercent}.`
          : `Custom processor state applied as ${nextProcessorState.min}/${nextProcessorState.max}.`,
      )
    } catch (error) {
      if (revision !== customPowerApplyRevisionRef.current) {
        return
      }

      setStatusMessage(`Custom processor apply failed: ${describeError(error)}`)
    }
  }

  async function handleFanProfile(profileId: FanProfile['id']) {
    const profileName = fanProfiles.find((profile) => profile.id === profileId)?.name ?? 'Fan'
    const hasService = serviceConnectedRef.current
    const hasFanHardware = hasService && (hardwareReadiness?.ready ?? true)
    const previousFanProfile =
      liveControlSnapshotRef.current?.activeFanProfile ?? activeFanProfile
    const toastId = showToast('info', 'Applying fan mode', `${profileName} requested.`, 0)

    setActiveFanProfile(profileId)
    setStatusMessage(
      hasFanHardware
        ? `${profileName} fan mode apply requested.`
        : hardwareReadiness && !hardwareReadiness.ready
          ? `${profileName} fan mode saved locally. Hardware fan writes are blocked: ${hardwareReadiness.detail}`
          : `${profileName} fan mode staged in the frontend preview.`,
    )
    pulseControl(profileId)
    queuePerformanceEvent('fan-profile-select', profileId, {
      serviceConnected: hasService,
    })

    if (!hasService) {
      void requestServiceHeal()
        .then((result) => {
          pushDebugEventRef.current(result.detail)
          if (result.requested || result.serviceRunning) {
            window.setTimeout(() => void refreshHardwareReadiness(true), 2500)
          }
        })
        .catch((error) => {
          pushDebugEventRef.current(`background service heal failed: ${describeError(error)}`)
        })
    }

    if (!hasFanHardware) {
      await waitForNextPaint()
      await persistStagedControlsRef.current({ activeFanProfile: profileId })
      updateToast(
        toastId,
        'warning',
        'Fan saved locally',
        hardwareReadiness && !hardwareReadiness.ready
          ? hardwareReadiness.detail
          : `${profileName} will apply when the service is available.`,
      )
      return
    }

    if (fanProfileApplyInFlightRef.current) {
      queuedFanProfileRef.current = profileId
      setStatusMessage(`${profileName} fan mode queued after the current hardware apply finishes.`)
      updateToast(toastId, 'info', 'Fan queued', `${profileName} will apply after the current request.`)
      queuePerformanceEvent('fan-profile-apply-queued', profileId)
      await waitForNextPaint()
      return
    }

    fanProfileApplyInFlightRef.current = true
    const applyStartedMs = performance.now()
    beginControlApply()
    queuePerformanceEvent('fan-profile-apply-started', profileId)

    try {
      await waitForNextPaint()
      const applyRequest =
        profileId === 'custom'
          ? applyCustomFanCurves(toBackendCurveSet(customCurvesRef.current))
          : applyFanProfile(profileId)
      const result = await withTimeout(
        applyRequest,
        FAN_PROFILE_APPLY_TIMEOUT_MS,
        `${profileName} fan mode apply`,
      )
      const refreshed = await refreshCachedLiveControlsAfterApply()
      const liveControls = refreshed.liveControls
      const layerAttention = summarizeLayerAttention(liveControls?.lastFanLayerStatuses)
      const queuedProfile = queuedFanProfileRef.current
      const resultSuperseded = queuedProfile !== null && queuedProfile !== profileId

      if (!resultSuperseded) {
        applyAuthoritativeSnapshot(mergeControlsWithLiveSnapshot(result.controls, liveControls), 'fanProfile')
      }
      queuePerformanceEvent('fan-profile-apply-finished', profileId, {
        totalMs: performance.now() - applyStartedMs,
        refreshMs: refreshed.refreshMs,
        superseded: resultSuperseded,
        queuedProfile,
      })
      if (!resultSuperseded) {
        setStatusMessage(
          layerAttention
            ? `${profileName} fan mode applied with layer attention: ${layerAttention}.`
            : result.detail,
        )
        updateToast(
          toastId,
          layerAttention ? 'warning' : 'success',
          layerAttention ? 'Fan partially applied' : 'Fan applied',
          layerAttention ? `${profileName}: ${layerAttention}` : `${profileName} applied.`,
        )
      } else {
        updateToast(toastId, 'info', 'Fan superseded', `${profileName} was replaced by a newer request.`)
      }
    } catch (error) {
      setActiveFanProfile(previousFanProfile)
      queuePerformanceEvent('fan-profile-apply-failed', profileId, {
        totalMs: performance.now() - applyStartedMs,
        error: describeError(error),
      })
      setStatusMessage(`Fan profile apply failed: ${describeError(error)}`)
      updateToast(toastId, 'error', 'Fan failed', describeError(error))
    } finally {
      fanProfileApplyInFlightRef.current = false
      const queuedProfile = queuedFanProfileRef.current
      queuedFanProfileRef.current = null
      endControlApply()
      if (queuedProfile !== null && queuedProfile !== profileId) {
        queuePerformanceEvent('fan-profile-apply-draining-queued', queuedProfile, {
          previousProfile: profileId,
        })
        void handleFanProfile(queuedProfile)
      }
    }
  }

  function makePresetId(prefix: string) {
    return `${prefix}-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`
  }

  function askPresetName(label: string, fallback: string) {
    const name = window.prompt(label, fallback)?.trim()
    return name && name.length > 0 ? name.slice(0, 48) : null
  }

  function saveCurrentFanPreset() {
    const currentFanName =
      fanProfiles.find((profile) => profile.id === activeFanProfile)?.name ?? 'Fan'
    const name = askPresetName('Name this fan preset', `${currentFanName} preset`)
    if (!name) {
      return
    }

    const preset: FanControlPreset = {
      id: makePresetId('fan'),
      name,
      profileId: activeFanProfile,
      curves: toBackendCurveSet(customCurvesRef.current),
    }
    const nextFanPresets = [...fanPresetsRef.current, preset]
    commitFanPresets(nextFanPresets)
    void persistStagedControlsRef.current({ fanPresets: nextFanPresets })
    showToast('success', 'Fan preset saved', name)
  }

  function saveCurrentPowerPreset() {
    const currentPowerName =
      powerProfiles.find((profile) => profile.id === activePowerProfile)?.name ?? 'Power'
    const name = askPresetName('Name this power preset', `${currentPowerName} preset`)
    if (!name) {
      return
    }

    const preset: PowerControlPreset = {
      id: makePresetId('power'),
      name,
      profileId: activePowerProfile,
      customBase: customPowerBaseRef.current,
      processorState: {
        minPercent: customProcessorStateRef.current.min,
        maxPercent: customProcessorStateRef.current.max,
      },
    }
    const nextPowerPresets = [...powerPresetsRef.current, preset]
    commitPowerPresets(nextPowerPresets)
    void persistStagedControlsRef.current({ powerPresets: nextPowerPresets })
    showToast('success', 'Power preset saved', name)
  }

  function saveCurrentQuickPreset() {
    const name = askPresetName('Name this quick preset', 'My quick preset')
    if (!name) {
      return
    }

    const fanPreset: FanControlPreset = {
      id: makePresetId('fan'),
      name: `${name} fan`,
      profileId: activeFanProfile,
      curves: toBackendCurveSet(customCurvesRef.current),
    }
    const powerPreset: PowerControlPreset = {
      id: makePresetId('power'),
      name: `${name} power`,
      profileId: activePowerProfile,
      customBase: customPowerBaseRef.current,
      processorState: {
        minPercent: customProcessorStateRef.current.min,
        maxPercent: customProcessorStateRef.current.max,
      },
    }
    const quickPreset: QuickControlPreset = {
      id: makePresetId('quick'),
      name,
      fanPresetId: fanPreset.id,
      powerPresetId: powerPreset.id,
    }
    const nextFanPresets = [...fanPresetsRef.current, fanPreset]
    const nextPowerPresets = [...powerPresetsRef.current, powerPreset]
    const nextQuickPresets = [...quickPresetsRef.current, quickPreset]
    commitFanPresets(nextFanPresets)
    commitPowerPresets(nextPowerPresets)
    commitQuickPresets(nextQuickPresets)
    void persistStagedControlsRef.current({
      fanPresets: nextFanPresets,
      powerPresets: nextPowerPresets,
      quickPresets: nextQuickPresets,
    })
    showToast('success', 'Quick preset saved', name)
  }

  function renamePreset(collection: 'fan' | 'power' | 'quick', id: string) {
    const presets =
      collection === 'fan'
        ? fanPresetsRef.current
        : collection === 'power'
          ? powerPresetsRef.current
          : quickPresetsRef.current
    const preset = presets.find((item) => item.id === id)
    if (!preset) {
      return
    }

    const name = askPresetName('Rename preset', preset.name)
    if (!name) {
      return
    }

    if (collection === 'fan') {
      const next = fanPresetsRef.current.map((item) =>
        item.id === id ? { ...item, name } : item,
      )
      commitFanPresets(next)
      void persistStagedControlsRef.current({ fanPresets: next })
    } else if (collection === 'power') {
      const next = powerPresetsRef.current.map((item) =>
        item.id === id ? { ...item, name } : item,
      )
      commitPowerPresets(next)
      void persistStagedControlsRef.current({ powerPresets: next })
    } else {
      const next = quickPresetsRef.current.map((item) =>
        item.id === id ? { ...item, name } : item,
      )
      commitQuickPresets(next)
      void persistStagedControlsRef.current({ quickPresets: next })
    }
    showToast('success', 'Preset renamed', name)
  }

  function duplicatePreset(collection: 'fan' | 'power' | 'quick', id: string) {
    if (collection === 'fan') {
      const preset = fanPresetsRef.current.find((item) => item.id === id)
      if (!preset) {
        return
      }
      const next = [
        ...fanPresetsRef.current,
        { ...preset, id: makePresetId('fan'), name: `${preset.name} copy` },
      ]
      commitFanPresets(next)
      void persistStagedControlsRef.current({ fanPresets: next })
      showToast('success', 'Fan preset duplicated', preset.name)
    } else if (collection === 'power') {
      const preset = powerPresetsRef.current.find((item) => item.id === id)
      if (!preset) {
        return
      }
      const next = [
        ...powerPresetsRef.current,
        { ...preset, id: makePresetId('power'), name: `${preset.name} copy` },
      ]
      commitPowerPresets(next)
      void persistStagedControlsRef.current({ powerPresets: next })
      showToast('success', 'Power preset duplicated', preset.name)
    } else {
      const preset = quickPresetsRef.current.find((item) => item.id === id)
      if (!preset) {
        return
      }
      const next = [
        ...quickPresetsRef.current,
        { ...preset, id: makePresetId('quick'), name: `${preset.name} copy` },
      ]
      commitQuickPresets(next)
      void persistStagedControlsRef.current({ quickPresets: next })
      showToast('success', 'Quick preset duplicated', preset.name)
    }
  }

  function deletePreset(collection: 'fan' | 'power' | 'quick', id: string) {
    if (collection === 'fan') {
      const nextFanPresets = fanPresetsRef.current.filter((preset) => preset.id !== id)
      const nextQuickPresets = quickPresetsRef.current
        .map((preset) =>
          preset.fanPresetId === id ? { ...preset, fanPresetId: null } : preset,
        )
        .filter((preset) => preset.fanPresetId || preset.powerPresetId)
      commitFanPresets(nextFanPresets)
      commitQuickPresets(nextQuickPresets)
      void persistStagedControlsRef.current({
        fanPresets: nextFanPresets,
        quickPresets: nextQuickPresets,
      })
    } else if (collection === 'power') {
      const nextPowerPresets = powerPresetsRef.current.filter((preset) => preset.id !== id)
      const nextQuickPresets = quickPresetsRef.current
        .map((preset) =>
          preset.powerPresetId === id ? { ...preset, powerPresetId: null } : preset,
        )
        .filter((preset) => preset.fanPresetId || preset.powerPresetId)
      commitPowerPresets(nextPowerPresets)
      commitQuickPresets(nextQuickPresets)
      void persistStagedControlsRef.current({
        powerPresets: nextPowerPresets,
        quickPresets: nextQuickPresets,
      })
    } else {
      const nextQuickPresets = quickPresetsRef.current.filter((preset) => preset.id !== id)
      commitQuickPresets(nextQuickPresets)
      void persistStagedControlsRef.current({ quickPresets: nextQuickPresets })
    }
    showToast('success', 'Preset deleted', 'Preset list updated.')
  }

  function applyFanPreset(preset: FanControlPreset) {
    const curves = fromBackendCurveSet(preset.curves)
    commitCustomCurves(curves)
    if (preset.profileId === 'custom') {
      void finalizeCustomCurveEditRef.current(curves, {
        activateCustom: true,
        statusMessage: `${preset.name} fan preset loaded.`,
      })
    } else {
      void persistStagedControlsRef.current({ customCurves: curves })
      void handleFanProfile(preset.profileId)
    }
    showToast('info', 'Fan preset applying', preset.name)
  }

  function applyPowerPreset(preset: PowerControlPreset) {
    const nextProcessorState = {
      min: preset.processorState.minPercent,
      max: preset.processorState.maxPercent,
    }
    customProcessorStateRef.current = nextProcessorState
    customPowerBaseRef.current = preset.customBase
    setCustomProcessorState(nextProcessorState)
    setCustomPowerBase(preset.customBase)
    void persistStagedControlsRef.current({
      customProcessorState: nextProcessorState,
      customPowerBase: preset.customBase,
    })
    void handlePowerProfile(preset.profileId)
    showToast('info', 'Power preset applying', preset.name)
  }

  function applyQuickPreset(preset: QuickControlPreset) {
    const powerPreset = powerPresetsRef.current.find((item) => item.id === preset.powerPresetId)
    const fanPreset = fanPresetsRef.current.find((item) => item.id === preset.fanPresetId)
    if (powerPreset) {
      applyPowerPreset(powerPreset)
    }
    if (fanPreset) {
      applyFanPreset(fanPreset)
    }
    showToast('info', 'Quick preset applying', preset.name)
  }

  async function applyCustomCurvesToService(nextCurves: CurveSet) {
    const result = await applyCustomFanCurves(toBackendCurveSet(nextCurves))
    const liveControls = await getLiveControlSnapshot().catch(() => null)

    applyAuthoritativeSnapshot(result.controls, 'fanCurves')
    updateSerializedState(liveControlSnapshotStateRef, liveControls, (next) => {
      liveControlSnapshotRef.current = next
      setLiveControlSnapshot(next)
    })
    return result.detail
  }

  async function finalizeCustomCurveEdit(
    nextCurves: CurveSet,
    options?: FinalizeCustomCurveOptions,
  ) {
    const nextActiveFanProfile = options?.activateCustom ? 'custom' : activeFanProfile
    const nextFanSyncLockEnabled = options?.fanSyncLockState ?? fanSyncLockEnabled
    const shouldPreviewCustom = options?.activateCustom && !fanHardwareApplyReady

    if (shouldPreviewCustom) {
      setActiveFanProfile('custom')
      pulseControl('custom')
    }

    await persistStagedControlsRef.current({
      activeFanProfile: nextActiveFanProfile,
      customCurves: nextCurves,
      fanSyncLockEnabled: nextFanSyncLockEnabled,
    })

    if (!fanHardwareApplyReady) {
      setStatusMessage(
        options?.statusMessage ??
          (hardwareReadiness && !hardwareReadiness.ready
            ? `Custom fan curves saved locally. Hardware fan writes are blocked: ${hardwareReadiness.detail}`
            : 'Custom fan curves saved to the preview. Connect the AeroForge service to apply them on hardware.'),
      )
      return
    }

    if (nextActiveFanProfile !== 'custom') {
      if (options?.statusMessage) {
        setStatusMessage(options.statusMessage)
      }
      return
    }

    try {
      const detail = await applyCustomCurvesToService(nextCurves)
      setStatusMessage(options?.statusMessage ?? detail)
    } catch (error) {
      setStatusMessage(`Custom fan curve apply failed: ${describeError(error)}`)
    }
  }
  finalizeCustomCurveEditRef.current = finalizeCustomCurveEdit
  handlePowerProfileRef.current = handlePowerProfile
  handleFanProfileRef.current = handleFanProfile
  copyDiagnosticsToClipboardRef.current = copyDiagnosticsToClipboard

  function cloneToCustom() {
    const nextCurves = fanSyncLockEnabled
      ? mirrorCurveSetFromTarget(activePreset, 'cpu')
      : duplicateCurveSet(activePreset)
    commitCustomCurves(nextCurves)
    void finalizeCustomCurveEdit(nextCurves, {
      activateCustom: true,
      statusMessage: 'Copied the active preset into Custom.',
    })
  }

  function applyFanCurveTemplate(template: FanCurveTemplate) {
    const nextCurves = fanSyncLockEnabled
      ? mirrorCurveSetFromTarget(template.curves, 'cpu')
      : duplicateCurveSet(template.curves)
    commitCustomCurves(nextCurves)
    pulseControl('custom')
    void finalizeCustomCurveEdit(nextCurves, {
      activateCustom: true,
      statusMessage: `Loaded the ${template.name} curve template into Custom.`,
    })
    showToast('success', `${template.name} curve loaded`, template.summary)
  }

  function resetCustomCurve() {
    const nextCurves = duplicateCurveSet(presetCurves.custom)
    commitCustomCurves(nextCurves)
    void finalizeCustomCurveEdit(nextCurves, {
      statusMessage: 'Custom fan curves restored to the default baseline.',
    })
  }

  function syncCurve(target: CurveTarget, source: CurveTarget) {
    const nextCurves = fanSyncLockEnabled
      ? mirrorCurveSetFromTarget(customCurvesRef.current, source)
      : {
          ...duplicateCurveSet(customCurvesRef.current),
          [target]: customCurvesRef.current[source].map((point) => ({ ...point })),
        }

    commitCustomCurves(nextCurves)
    void finalizeCustomCurveEdit(nextCurves, {
      statusMessage: fanSyncLockEnabled
        ? `Sync Lock kept both curves linked to ${source.toUpperCase()}.`
        : `${target.toUpperCase()} curve mirrored from ${source.toUpperCase()}.`,
    })
  }

  function toggleFanSyncLock(source: CurveTarget) {
    const nextEnabled = !fanSyncLockEnabled
    setFanSyncLockEnabled(nextEnabled)

    if (!nextEnabled) {
      setStatusMessage('Sync Lock disabled. CPU and GPU curves can diverge again.')
      void persistStagedControlsRef.current({ fanSyncLockEnabled: false })
      return
    }

    const nextCurves = mirrorCurveSetFromTarget(customCurvesRef.current, source)
    commitCustomCurves(nextCurves)
    void finalizeCustomCurveEdit(nextCurves, {
      fanSyncLockState: true,
      statusMessage: `Sync Lock enabled. ${source.toUpperCase()} now drives both curves.`,
    })
  }

  async function handleBootFile(event: ChangeEvent<HTMLInputElement>) {
    if (!bootLogoWritable || settingsActionPending) {
      setStatusMessage(bootLogoDisabledReason ?? 'Boot-logo replacement is disabled.')
      event.target.value = ''
      return
    }

    const file = event.target.files?.[0]
    if (!file) {
      return
    }

    if (customBootPreview) {
      URL.revokeObjectURL(customBootPreview)
    }

    const objectUrl = URL.createObjectURL(file)
    setCustomBootPreview(objectUrl)
    setCustomBootFilename(buildJpegBootLogoName(file.name))
    setSelectedBootArt('custom')
    pulseControl('boot-upload')

    setSettingsActionPending('boot-logo')
    setStatusMessage('Preparing and applying the boot-logo image...')

    try {
      const prepared = await prepareBootLogoUpload(file)
      const result = await applyBootLogo(prepared.fileName, prepared.imageBase64, 'custom')
      applyAuthoritativeSnapshot(result.controls, 'bootLogo')
      await persistStagedControlsRef.current({
        selectedBootArt: 'custom',
        customBootFilename: prepared.fileName,
      })
      setStatusMessage(result.detail)
    } catch (error) {
      setStatusMessage(`Boot-logo apply failed: ${describeError(error)}`)
    } finally {
      setSettingsActionPending((current) => (current === 'boot-logo' ? null : current))
      event.target.value = ''
    }
  }

  async function handleBootArtworkApply(art: BootArt) {
    if (!bootLogoWritable || settingsActionPending) {
      setStatusMessage(bootLogoDisabledReason ?? 'Boot-logo replacement is disabled.')
      return
    }

    setSelectedBootArt(art.id)
    pulseControl('boot-upload')
    setSettingsActionPending('boot-logo')
    setStatusMessage(`Preparing ${art.name} for boot-logo apply...`)

    try {
      const prepared = await preparePresetBootLogo(art)
      const result = await applyBootLogo(
        prepared.fileName,
        prepared.imageBase64,
        art.id as BootArtId,
      )
      applyAuthoritativeSnapshot(result.controls, 'bootLogo')
      await persistStagedControlsRef.current({
        selectedBootArt: art.id,
        customBootFilename: prepared.fileName,
      })
      setStatusMessage(result.detail)
    } catch (error) {
      setStatusMessage(`Boot-logo apply failed: ${describeError(error)}`)
    } finally {
      setSettingsActionPending((current) => (current === 'boot-logo' ? null : current))
    }
  }

  async function notifyWindowsUpdateAvailable(status: UpdateStatus) {
    const notificationKey =
      status.latestVersion ?? status.latestCommitSha ?? status.latestTitle ?? null

    if (
      !isDesktopRuntime() ||
      !status.updateAvailable ||
      !status.canStageUpdate ||
      !notificationKey ||
      updateNotificationKeyRef.current === notificationKey
    ) {
      return
    }

    const previousNotificationKey = updateNotificationKeyRef.current
    updateNotificationKeyRef.current = notificationKey

    const versionLabel = status.latestVersion
      ? `v${status.latestVersion}`
      : status.latestTitle ?? 'New AeroForge release'

    try {
      await showUpdateNotification(versionLabel)
    } catch (error) {
      updateNotificationKeyRef.current = previousNotificationKey
      pushTransportDebugEventRef.current(
        `windows update notification failed: ${describeError(error)}`,
      )
    }
  }

  async function runUpdateCheck(manual: boolean, channelOverride?: UpdateChannel) {
    setUpdateActionPending('check')
    setUpdateActionMessage('Checking the published GitHub release feed...')

    try {
      const status = await checkForUpdates(channelOverride ?? updateChannel)
      setUpdateStatus(status)
      setUpdateActionMessage(status.detail)
      void notifyWindowsUpdateAvailable(status)
      if (manual || status.updateAvailable || status.lastError) {
        setStatusMessage(status.detail)
      }
      return status
    } catch (error) {
      // The failure is surfaced through the status line; callers fire-and-forget,
      // so rethrowing here would only produce an unhandled rejection.
      const message = describeError(error)
      const detail = `Update check failed: ${message}`
      setUpdateActionMessage(detail)
      setStatusMessage(detail)
      return null
    } finally {
      setUpdateActionPending(null)
    }
  }
  runUpdateCheckRef.current = runUpdateCheck

  async function handleStageLatestUpdate() {
    setUpdateActionPending('stage')
    setUpdateActionMessage('Downloading the update package and verifying it...')

    try {
      const status = await stageUpdateDownload(updateChannel)
      setUpdateStatus(status)
      setUpdateActionMessage(status.detail)
      setStatusMessage(status.detail)
    } catch (error) {
      const detail = `Update download failed: ${describeError(error)}`
      setUpdateActionMessage(detail)
      setStatusMessage(detail)
    } finally {
      setUpdateActionPending(null)
    }
  }

  async function handleInstallLatestUpdate() {
    setUpdateActionPending('install')
    setUpdateActionMessage('Launching the staged updater...')
    const stagedAssetName = updateStatus?.stagedAssetName?.toLowerCase() ?? ''
    const shouldCloseForPortableUpdate = stagedAssetName.endsWith('.zip')

    try {
      const status = await installStagedUpdate()
      setUpdateStatus(status)
      setUpdateActionMessage(status.detail)
      setStatusMessage(status.detail)
      if (shouldCloseForPortableUpdate) {
        await new Promise<void>((resolve) => window.setTimeout(resolve, 1400))
        await getCurrentWindow().close()
      }
    } catch (error) {
      const detail = `Staged install failed: ${describeError(error)}`
      setUpdateActionMessage(detail)
      setStatusMessage(detail)
    } finally {
      setUpdateActionPending(null)
    }
  }

  function handleToggleUpdateChecksOnLaunch() {
    setCheckForUpdatesOnLaunch((current) => {
      const next = !current
      setStatusMessage(
        next
          ? 'Launch-time update checks enabled.'
          : 'Launch-time update checks disabled. Manual checks stay available.',
      )
      return next
    })
  }

  async function handleBlueLightFilterToggle() {
    if (settingsActionPending || !blueLightWritable) {
      if (!blueLightWritable && blueLightDisabledReason) {
        setStatusMessage(blueLightDisabledReason)
      }
      return
    }

    const previousEnabled = blueLightFilterEnabled
    const nextEnabled = !previousEnabled

    setSettingsActionPending('blue-light')
    commitBlueLightFilterEnabled(nextEnabled)
    setStatusMessage(
      nextEnabled
        ? 'Applying the Acer eye-care gamma ramp...'
        : 'Restoring the neutral display gamma ramp...',
    )

    try {
      const result = await applyBlueLightFilter(nextEnabled)
      await persistStagedControlsRef.current({ blueLightFilterEnabled: nextEnabled })
      setStatusMessage(result.detail)
    } catch (error) {
      commitBlueLightFilterEnabled(previousEnabled)
      setStatusMessage(`Blue light filter apply failed: ${describeError(error)}`)
    } finally {
      setSettingsActionPending((current) => (current === 'blue-light' ? null : current))
    }
  }

  async function handleAutoRefreshRateToggle() {
    if (settingsActionPending) {
      return
    }

    const previousEnabled = autoRefreshRateOnBatteryEnabledRef.current
    const previousRestoreHz = autoRefreshRateRestoreHzRef.current
    const nextEnabled = !previousEnabled
    const onBattery = displayedAcPluggedIn === false

    setSettingsActionPending('refresh-rate')
    commitAutoRefreshRateSettings(nextEnabled, previousRestoreHz)
    autoRefreshRateSyncKeyRef.current = `${nextEnabled}:${onBattery}:${
      previousRestoreHz ?? 'none'
    }`
    setStatusMessage(
      nextEnabled
        ? 'Arming automatic 60 Hz display mode for battery power...'
        : 'Disabling automatic 60 Hz display mode and restoring the saved refresh rate...',
    )

    try {
      const result = await syncAutoRefreshRateState(nextEnabled, onBattery, true)
      autoRefreshRateSyncKeyRef.current = `${result.enabled}:${result.onBattery}:${
        result.restoreHz ?? 'none'
      }`
    } catch (error) {
      commitAutoRefreshRateSettings(previousEnabled, previousRestoreHz)
      autoRefreshRateSyncKeyRef.current = null
      setStatusMessage(`Auto 60 Hz apply failed: ${describeError(error)}`)
    } finally {
      setSettingsActionPending((current) => (current === 'refresh-rate' ? null : current))
    }
  }

  async function handleSmartChargingMode(nextEnabled: boolean) {
    if (settingsActionPending || nextEnabled === smartChargingEnabled || !smartChargeWritable) {
      if (!smartChargeWritable && smartChargeDisabledReason) {
        setStatusMessage(smartChargeDisabledReason)
      }
      return
    }

    const previousEnabled = smartChargingEnabled

    setSettingsActionPending('smart-charge')
    commitSmartChargingEnabled(nextEnabled)
    setStatusMessage(
      nextEnabled
        ? 'Applying optimized battery charging through Acer battery control...'
        : 'Clearing the battery-health cap for full charging...',
    )

    try {
      const result = await applySmartCharging(nextEnabled)
      await persistStagedControlsRef.current({ smartChargingEnabled: nextEnabled })
      setStatusMessage(result.detail)
      pulseControl('charge-toggle')
    } catch (error) {
      commitSmartChargingEnabled(previousEnabled)
      setStatusMessage(`Smart charge apply failed: ${describeError(error)}`)
    } finally {
      setSettingsActionPending((current) => (current === 'smart-charge' ? null : current))
    }
  }

  async function handleProcessorStateControlToggle() {
    const nextEnabled = !processorStateControlEnabledRef.current

    if (!nextEnabled && customPowerApplyTimerRef.current !== null) {
      window.clearTimeout(customPowerApplyTimerRef.current)
      customPowerApplyTimerRef.current = null
      customPowerApplyRevisionRef.current += 1
    }

    commitProcessorStateControlEnabled(nextEnabled)
    await persistStagedControlsRef.current({ processorStateControlEnabled: nextEnabled })
    setStatusMessage(
      nextEnabled
        ? 'CPU min/max writes enabled. Power modes can update Windows processor policy again.'
        : 'CPU min/max writes disabled. Firmware power modes still apply.',
    )
  }

  async function handleNvidiaTelemetryToggle() {
    if (settingsActionPending) {
      return
    }

    const previousEnabled = nvidiaTelemetryEnabledRef.current
    const nextEnabled = !previousEnabled

    commitNvidiaTelemetryEnabled(nextEnabled)
    setSettingsActionPending('nvidia-telemetry')
    setStatusMessage(
      nextEnabled
        ? 'Re-enabling NVIDIA telemetry polling...'
        : 'Disabling NVIDIA telemetry polling so the dGPU can idle...',
    )

    try {
      const result = await setNvidiaTelemetryEnabled(nextEnabled)
      applyAuthoritativeSnapshot(result.controls, 'nvidiaTelemetry')
      setStatusMessage(result.detail)
    } catch (error) {
      commitNvidiaTelemetryEnabled(previousEnabled)
      setStatusMessage(`NVIDIA telemetry setting failed: ${describeError(error)}`)
    } finally {
      setSettingsActionPending((current) => (current === 'nvidia-telemetry' ? null : current))
    }
  }

  async function handleHardwareRepair() {
    if (settingsActionPending || hardwareReadiness?.repairAvailable === false) {
      return
    }

    setSettingsActionPending('hardware-repair')
    setStatusMessage('Launching AeroForge hardware repair. Approve the Windows prompt to continue.')
    const toastId = showToast(
      'info',
      'Repair starting',
      'Approve the Windows administrator prompt once.',
      0,
    )
    queuePerformanceEvent('hardware-repair-launch', hardwareReadiness?.detail ?? 'unknown')

    try {
      const result = await repairHardwarePrerequisites()
      setStatusMessage(result.detail)
      updateToast(
        toastId,
        result.launched ? 'success' : 'error',
        result.launched ? 'Repair launched' : 'Repair unavailable',
        result.detail,
      )
      pushDebugEvent(result.detail)
      await refreshHardwareReadiness(true)
      if (result.launched) {
        window.setTimeout(() => {
          void refreshHardwareReadiness(true)
        }, 5000)
      }
    } catch (error) {
      setStatusMessage(`Hardware repair launch failed: ${describeError(error)}`)
      updateToast(toastId, 'error', 'Repair failed', describeError(error))
    } finally {
      setSettingsActionPending((current) => (current === 'hardware-repair' ? null : current))
    }
  }

  async function handleHardwareServiceStart() {
    if (settingsActionPending || !hardwareReadiness?.service.installed) {
      return
    }

    setSettingsActionPending('hardware-start')
    setStatusMessage('Starting AeroForge service in the background...')
    const toastId = showToast(
      'info',
      'Starting service',
      'AeroForge is asking the background healer to start hardware control.',
      0,
    )
    queuePerformanceEvent('hardware-service-start', hardwareReadiness?.detail ?? 'unknown')

    try {
      const result = await requestServiceHeal()
      setStatusMessage(result.detail)
      updateToast(
        toastId,
        result.serviceRunning ? 'success' : result.requested ? 'warning' : 'error',
        result.serviceRunning ? 'Service started' : result.requested ? 'Still starting' : 'Start blocked',
        result.detail,
      )
      pushDebugEvent(result.detail)
      await refreshHardwareReadiness(true)
      if (result.requested && !result.serviceRunning) {
        window.setTimeout(() => {
          void refreshHardwareReadiness(true)
        }, 5000)
      }
    } catch (error) {
      setStatusMessage(`AeroForge service start failed: ${describeError(error)}`)
      updateToast(toastId, 'error', 'Service start failed', describeError(error))
    } finally {
      setSettingsActionPending((current) => (current === 'hardware-start' ? null : current))
    }
  }

  function updateCustomProcessorState(
    field: 'min' | 'max',
    rawValue: number,
  ) {
    const lowerBound = field === 'min' ? 0 : 5
    const value = clamp(Math.round(rawValue), lowerBound, 100)
    const current = customProcessorStateRef.current
    const nextState =
      field === 'min'
        ? {
            min: Math.min(value, current.max),
            max: Math.max(current.max, Math.min(value, current.max)),
          }
        : {
            min: Math.min(current.min, Math.max(value, current.min)),
            max: Math.max(value, current.min),
          }

    customProcessorStateRef.current = nextState
    setCustomProcessorState(nextState)

    setActivePowerProfile('custom')
    void persistStagedControlsRef.current({
      activePowerProfile: 'custom',
      customProcessorState: nextState,
    })
    scheduleCustomPowerApply(
      nextState,
      customPowerBaseRef.current,
      `Custom processor state updated to ${nextState.min}% minimum / ${nextState.max}% maximum`,
    )
    pulseControl('custom')
  }

  function updateCustomPowerBase(nextBase: CustomPowerBaseId) {
    if (nextBase === customPowerBaseRef.current) {
      return
    }

    customPowerBaseRef.current = nextBase
    setCustomPowerBase(nextBase)
    setActivePowerProfile('custom')
    void persistStagedControlsRef.current({
      activePowerProfile: 'custom',
      customPowerBase: nextBase,
    })

    const selectedBase =
      customPowerBaseOptions.find((option) => option.id === nextBase)?.name ?? nextBase
    scheduleCustomPowerApply(
      customProcessorStateRef.current,
      nextBase,
      `Custom firmware base changed to ${selectedBase}`,
    )
    pulseControl('custom')
  }

  function updateGpuOverclockSetting(
    field: keyof GpuTuningState,
    value: number,
  ) {
    setGpuOverclock((current) => ({
      ...current,
      [field]: value,
    }))
    setActivePowerProfile('custom')
    setOcApplyState('staged')
    pulseControl('oc-apply')
  }

  function handleOcProfileSlot(slotId: string) {
    const slot = ocProfileSlots.find((item) => item.id === slotId)
    if (!slot) {
      return
    }

    setActivePowerProfile('custom')
    setActiveOcSlot(slot.id)
    setGpuOverclock({ ...slot.settings })
    setOcApplyState('staged')
    setStatusMessage(`${slot.name} loaded into the GPU tuning page.`)
    pulseControl(slot.id)
  }

  async function handleApplyGpuTuning() {
    pulseControl('oc-apply')

    if (!serviceConnected) {
      setOcApplyState('live')
      setStatusMessage(
        `Marked ${currentOcSlot.name} live in the preview only. Connect the AeroForge service to apply the GPU tuning on hardware.`,
      )
      return
    }

    try {
      const result = await applyGpuTuning(
        toBackendGpuTuningState(gpuOverclock),
        activeOcSlot,
      )

      applyAuthoritativeSnapshot(result.controls, 'gpu')

      setStatusMessage(result.detail)
    } catch (error) {
      setStatusMessage(`GPU tuning apply failed: ${describeError(error)}`)
    }
  }

  async function handleSaveGpuTuning() {
    const savedCustomSlot: OcProfileSlot = {
      ...customOcSlot,
      strap: buildCustomOcStrap(gpuOverclock),
      settings: { ...gpuOverclock },
    }

    setCustomOcSlot(savedCustomSlot)
    setActiveOcSlot(savedCustomSlot.id)
    setOcApplyState('staged')
    pulseControl(savedCustomSlot.id)

    try {
      await saveControlSnapshot(
        buildControlSnapshotForPersistence({
          activePowerProfile: 'custom',
          activeFanProfile,
          customProcessorState,
          customPowerBase,
          gpuOverclock,
          ocProfileSlots: [...builtInOcProfileSlots, savedCustomSlot],
          activeOcSlot: savedCustomSlot.id,
          ocApplyState: 'staged',
          ocTuningLocked,
          customCurves: customCurvesRef.current,
          fanPresets: fanPresetsRef.current,
          powerPresets: powerPresetsRef.current,
          quickPresets: quickPresetsRef.current,
          fanSyncLockEnabled,
          smartChargingEnabled: smartChargingEnabledRef.current,
          processorStateControlEnabled: processorStateControlEnabledRef.current,
          nvidiaTelemetryEnabled: nvidiaTelemetryEnabledRef.current,
          autoRefreshRateOnBatteryEnabled: autoRefreshRateOnBatteryEnabledRef.current,
          autoRefreshRateRestoreHz: autoRefreshRateRestoreHzRef.current,
          usbPowerEnabled,
          blueLightFilterEnabled: blueLightFilterEnabledRef.current,
          selectedBootArt,
          customBootFilename,
          updateChannel,
          checkForUpdatesOnLaunch,
        }),
      )

      setStatusMessage(`Saved the current GPU tuning into ${savedCustomSlot.name}.`)
    } catch (error) {
      setStatusMessage(
        `Saved the current GPU tuning locally, but backend persistence failed: ${describeError(error)}`,
      )
    }
  }

  async function handleToggleOcLock() {
    const nextLocked = !ocTuningLocked

    setOcTuningLocked(nextLocked)
    setStatusMessage(
      nextLocked
        ? 'GPU tuning locked to prevent accidental slider changes.'
        : 'GPU tuning unlocked for further edits in the preview.',
    )

    try {
      await saveControlSnapshot(
        buildControlSnapshotForPersistence({
          activePowerProfile,
          activeFanProfile,
          customProcessorState,
          customPowerBase,
          gpuOverclock,
          ocProfileSlots,
          activeOcSlot,
          ocApplyState,
          ocTuningLocked: nextLocked,
          customCurves: customCurvesRef.current,
          fanPresets: fanPresetsRef.current,
          powerPresets: powerPresetsRef.current,
          quickPresets: quickPresetsRef.current,
          fanSyncLockEnabled,
          smartChargingEnabled: smartChargingEnabledRef.current,
          processorStateControlEnabled: processorStateControlEnabledRef.current,
          nvidiaTelemetryEnabled: nvidiaTelemetryEnabledRef.current,
          autoRefreshRateOnBatteryEnabled: autoRefreshRateOnBatteryEnabledRef.current,
          autoRefreshRateRestoreHz: autoRefreshRateRestoreHzRef.current,
          usbPowerEnabled,
          blueLightFilterEnabled: blueLightFilterEnabledRef.current,
          selectedBootArt,
          customBootFilename,
          updateChannel,
          checkForUpdatesOnLaunch,
        }),
      )
    } catch (error) {
      setStatusMessage(
        `Tuner lock changed locally, but backend persistence failed: ${describeError(error)}`,
      )
    }
  }

  function handleResetGpuTuning() {
    setActiveOcSlot('daily')
    setGpuOverclock({ ...defaultGpuOverclock })
    setOcApplyState('staged')
    setStatusMessage(
      'GPU overclocking and voltage controls restored to the AeroForge daily baseline.',
    )
    pulseControl('daily')
  }

  const activeStageFitSnapshot =
    activeTab === 'home' || activeTab === 'power' || activeTab === 'fans'
      ? stageFitSnapshots[activeTab]
      : stageFitSnapshots.home ?? stageFitSnapshots.power ?? stageFitSnapshots.fans
  const stageFitDetail =
    (['home', 'power', 'fans'] as const)
      .map((tab) => formatStageFitValue(stageFitSnapshots[tab]))
      .filter(Boolean)
      .join(' | ') || 'Switch Home, Power, or Fans to capture scale data.'
  const activeStageFitDetail = formatStageFitDetail(activeStageFitSnapshot)

  return (
    <div className="shell">
      <main className="dashboard" ref={dashboardRef}>
        <header className="topbar" ref={topbarRef}>
          <div className="topbar__brand">
            <div className="topbar__brand-mark">
              <img src={aeroforgeMark} alt="" aria-hidden="true" />
            </div>
            <div className="topbar__brand-copy">
              <strong>AeroForge</strong>
              <span className="brand-subtitle">Control Center</span>
            </div>
          </div>

          <nav className="topbar__nav" aria-label="Primary">
            {navigationTabs.map((tab) => (
              <button
                key={tab.id}
                className={activeTab === tab.id ? 'is-active' : ''}
                onClick={() => setActiveTab(tab.id)}
              >
                {tab.label}
              </button>
            ))}
            <button
              type="button"
              className="topbar__shortcut-hint"
              title="Keyboard shortcuts (?)"
              aria-label="Show keyboard shortcuts"
              onClick={() => setShortcutsHelpOpen(true)}
            >
              ?
            </button>
          </nav>
        </header>

        {hardwareIssueVisible && hardwareReadiness && (
          <HardwareReadinessBanner
            title={hardwareIssueTitle}
            detail={hardwareIssueDetail}
            readiness={hardwareReadiness}
            repairPending={hardwareRepairPending}
            startPending={hardwareStartPending}
            onRepair={() => void handleHardwareRepair()}
            onStartService={() => void handleHardwareServiceStart()}
          />
        )}

        {(activeTab === 'home' || activeTab === 'power' || activeTab === 'fans') && (
          <>
            <PresetPanel
              fanPresets={fanPresets}
              powerPresets={powerPresets}
              quickPresets={quickPresets}
              onSaveFan={saveCurrentFanPreset}
              onSavePower={saveCurrentPowerPreset}
              onSaveQuick={saveCurrentQuickPreset}
              onApplyFan={applyFanPreset}
              onApplyPower={applyPowerPreset}
              onApplyQuick={applyQuickPreset}
              onRename={renamePreset}
              onDuplicate={duplicatePreset}
              onDelete={deletePreset}
              onExport={exportPresetPack}
              onImport={triggerPresetImport}
            />
            <input
              ref={presetImportInputRef}
              type="file"
              accept="application/json,.json"
              className="visually-hidden"
              onChange={(event) => void handlePresetImportFile(event)}
              aria-hidden
              tabIndex={-1}
            />
          </>
        )}

        {activeTab === 'home' && (
          <div
            className="home-stage-fit"
            style={{
              height: homeScaledHeight == null ? undefined : `${homeScaledHeight}px`,
            }}
          >
          <section
            className="home-stage panel"
            ref={homeStageRef}
            style={{
              transform: homeScale === 1 ? undefined : `scale(${homeScale})`,
            }}
          >
            <aside className="home-stage__sidebar">
              <div className="home-dial-card">
                <span className="eyebrow">Performance Core</span>
                <div className="home-dial">
                  <div className="home-dial__ring" />
                <div className="home-dial__content">
                  <span>GPU</span>
                  <strong>{formatTelemetryValue(displayedGpuClock)}</strong>
                  <small>{displayedGpuClock == null ? '' : 'MHz'}</small>
                </div>
              </div>

              <div className="home-dial-card__stats">
                <div>
                  <span className="eyebrow">GPU Usage</span>
                  <strong>{formatTelemetryValue(displayedGpuUsage, '%')}</strong>
                </div>
                <div>
                  <span className="eyebrow">CPU Usage</span>
                  <strong>{formatTelemetryValue(displayedCpuUsage, '%')}</strong>
                </div>
              </div>
            </div>

              <div className="home-shortcuts">
                <button className="home-shortcut" onClick={() => setActiveTab('power')}>
                  <span className="eyebrow">Power</span>
                  <strong>{currentPowerProfile.name}</strong>
                  <small>{currentPowerWattage}</small>
                </button>

                <button className="home-shortcut" onClick={() => setActiveTab('fans')}>
                  <span className="eyebrow">Fans</span>
                  <strong>{currentFanProfile.name}</strong>
                  <small>{displayedGpuFanRpm == null ? '' : `${displayedGpuFanRpm} RPM live GPU`}</small>
                </button>

                <button className="home-shortcut" onClick={() => setActiveTab('personal')}>
                  <span className="eyebrow">Settings</span>
                  <strong>{smartChargeTarget} charge cap</strong>
                  <small>
                    {selectedBootArt === 'custom'
                      ? customBootFilename
                      : currentBootArt?.name ?? 'Preset logo'}
                  </small>
                </button>
              </div>
            </aside>

            <section className="home-stage__focus">
              <span className="eyebrow">AeroForge Control</span>
              <h1>{powerHeadline}</h1>
              <p className="home-stage__subtitle">System Mode</p>

              <div className="home-stage__mode-strip">
                <span>{currentPowerProfile.summary}</span>
              </div>

              <div className="home-stage__actions">
                <button className="button button--home-action" onClick={() => setActiveTab('power')}>
                  Open Power Modes
                </button>
                <button className="button button--home-action" onClick={() => setActiveTab('fans')}>
                  Open Fan Control
                </button>
              </div>

              <div className="home-stage__metrics">
                <MetricCard
                  label="Runtime"
                  value={currentPowerRuntimeValue}
                  detail={currentPowerRuntimeDetail}
                />
                <MetricCard
                  label="Fan Mode"
                  value={currentFanProfile.name}
                  detail={fanTelemetryDescriptor.modeLabel}
                />
                <MetricCard
                  label="Charge Target"
                  value={smartChargeTarget}
                  detail={
                    serviceConnected
                      ? formatLiveBatteryDetail(displayedBatteryPercent)
                      : smartChargingEnabled
                        ? 'Wear-aware charging'
                        : 'Maximum capacity'
                  }
                />
              </div>

              <div className="home-stage__fan-readouts">
                <HomeFanReadoutCard
                  label="GPU Fan"
                  value={displayedGpuFanRpm}
                  detail={
                    buildFanSpeedDetail(
                      displayedGpuFanRpm,
                      displayedGpuFanTargetPercent,
                      serviceConnected
                        ? 'Live graphics cooling speed'
                        : 'Cached graphics cooling speed',
                    )
                  }
                />
                <HomeFanReadoutCard
                  label="CPU Fan"
                  value={displayedCpuFanRpm}
                  detail={
                    buildFanSpeedDetail(
                      displayedCpuFanRpm,
                      displayedCpuFanTargetPercent,
                      serviceConnected
                        ? 'Live processor cooling speed'
                        : 'Cached processor cooling speed',
                    )
                  }
                />
              </div>
            </section>

            <aside className="home-stage__telemetry">
              <HomeTemperatureDial
                label="GPU"
                identity={displayedGpuIdentity}
                value={displayedGpuTemp}
                details={[
                  { label: 'Util', value: displayedGpuUsage, suffix: '%' },
                  { label: 'VRAM', value: displayedGpuMemoryUsage, suffix: '%' },
                  {
                    label: 'Power',
                    value: displayedGpuPowerDraw,
                    displayValue: formatWattReadout(displayedGpuPowerDraw),
                  },
                ]}
              />
              <HomeTemperatureDial
                label="CPU"
                identity={displayedCpuIdentity}
                value={displayedCpuTemp}
                details={[
                  {
                    label: 'Power',
                    value: displayedCpuPackagePower,
                    displayValue: formatWattReadout(displayedCpuPackagePower),
                  },
                  { label: 'Min', value: displayedCpuTempLowest, suffix: ' C' },
                  { label: 'Max', value: displayedCpuTempHighest, suffix: ' C' },
                ]}
              />
              {displayedSystemTemp != null && (
                <HomeTemperatureDial
                  label="System"
                  identity={displayedSystemIdentity}
                  value={displayedSystemTemp}
                />
              )}
            </aside>
          </section>
          </div>
        )}

        <div className="dashboard__grid">
          {activeTab === 'fans' && (
            <div
              className="page-stage-fit"
              style={{
                height: fansScaledHeight == null ? undefined : `${fansScaledHeight}px`,
              }}
            >
            <section
              className="panel panel--wide fan-mode page-stage"
              ref={fansStageRef}
              style={{
                transform: fansScale === 1 ? undefined : `scale(${fansScale})`,
              }}
            >
              <div className="fan-mode__toolbar">
                <div className="fan-mode__field">
                  <span className="eyebrow">Fan Profile</span>
                  <div className="fan-mode__select">
                    <strong>{currentFanProfile.name}</strong>
                    <small>{currentFanProfile.strap}</small>
                  </div>
                </div>

                <div className="fan-mode__field fan-mode__field--meta">
                  <span className="eyebrow">Cooling Target</span>
                  <div className="fan-mode__meta-card">
                    <strong>{fanTelemetryDescriptor.modeLabel}</strong>
                  </div>
                </div>
              </div>

              <div className="fan-mode__tabs">
                <button className="is-active">Fan Control</button>
              </div>

              <LayerStatusStrip statuses={fanLayerStatuses} emptyLabel="No fan apply has run yet." />

              <div className="fan-mode__frame">
                <div className="fan-mode__header">
                  <span className="eyebrow">Fan Control</span>
                </div>

                <div className="fan-mode__profiles">
                  {fanProfiles.map((profile) => (
                    <button
                      key={profile.id}
                      className={`fan-tile ${
                        activeFanProfile === profile.id ? 'is-selected' : ''
                      } ${glowTarget === profile.id ? 'is-pulsing' : ''}`}
                      onClick={() => void handleFanProfile(profile.id)}
                    >
                      <div className="fan-tile__icon">{profile.badge}</div>
                      <strong>{profile.name}</strong>
                      <small>{profile.strap}</small>
                    </button>
                  ))}
                </div>

                {activeFanProfile === 'custom' ? (
                  <>
                    <CurveTemplateBar
                      templates={FAN_CURVE_TEMPLATES}
                      disabled={settingsActionPending !== null}
                      onApply={applyFanCurveTemplate}
                    />

                    <div className="fan-custom-grid">
                      <FanCurvePanel
                        title="GPU curve"
                        target="gpu"
                        points={customCurves.gpu}
                        editable
                        chartRef={(node) => {
                          chartRefs.current.gpu = node
                        }}
                        onPointDown={(index) => setDraggingPoint({ target: 'gpu', index })}
                        syncLockEnabled={fanSyncLockEnabled}
                        onSyncLockToggle={() => toggleFanSyncLock('gpu')}
                        onSecondaryAction={() => syncCurve('gpu', 'cpu')}
                        secondaryLabel="Sync from CPU"
                      />

                      <FanCurvePanel
                        title="CPU curve"
                        target="cpu"
                        points={customCurves.cpu}
                        editable
                        chartRef={(node) => {
                          chartRefs.current.cpu = node
                        }}
                        onPointDown={(index) => setDraggingPoint({ target: 'cpu', index })}
                        syncLockEnabled={fanSyncLockEnabled}
                        onSyncLockToggle={() => toggleFanSyncLock('cpu')}
                        onSecondaryAction={() => syncCurve('cpu', 'gpu')}
                        secondaryLabel="Sync from GPU"
                      />
                    </div>

                    <div className="fan-preset-dashboard fan-preset-dashboard--compact">
                      <div className="fan-rpm-card">
                        <div className="fan-rpm-card__fan fan-rpm-card__fan--gpu" />
                        <div className="fan-rpm-card__content">
                          <span className="eyebrow">GPU</span>
                          <strong>{formatTelemetryValue(displayedGpuFanRpm)}</strong>
                          <small>
                            {formatFanSpeedMeta(
                              displayedGpuFanRpm,
                              displayedGpuFanTargetPercent,
                            )}
                          </small>
                        </div>
                      </div>

                      <div className="fan-rpm-card">
                        <div className="fan-rpm-card__fan fan-rpm-card__fan--cpu" />
                        <div className="fan-rpm-card__content">
                          <span className="eyebrow">CPU</span>
                          <strong>{formatTelemetryValue(displayedCpuFanRpm)}</strong>
                          <small>
                            {formatFanSpeedMeta(
                              displayedCpuFanRpm,
                              displayedCpuFanTargetPercent,
                            )}
                          </small>
                        </div>
                      </div>
                    </div>

                    <div className="fan-custom-footer">
                      <button className="button button--ghost" onClick={resetCustomCurve}>
                        Reset custom
                      </button>
                      <span>
                        Custom mode stores edits immediately and refreshes hardware only when the curve target changes or a safety refresh is due.
                      </span>
                    </div>
                  </>
                ) : (
                  <>
                    <div className="fan-preset-dashboard">
                      <div className="fan-rpm-card">
                        <div className="fan-rpm-card__fan fan-rpm-card__fan--gpu" />
                        <div className="fan-rpm-card__content">
                          <span className="eyebrow">GPU</span>
                          <strong>{formatTelemetryValue(displayedGpuFanRpm)}</strong>
                          <small>
                            {formatFanSpeedMeta(
                              displayedGpuFanRpm,
                              displayedGpuFanTargetPercent,
                            )}
                          </small>
                        </div>
                      </div>

                      <div className="fan-rpm-card">
                        <div className="fan-rpm-card__fan fan-rpm-card__fan--cpu" />
                        <div className="fan-rpm-card__content">
                          <span className="eyebrow">CPU</span>
                          <strong>{formatTelemetryValue(displayedCpuFanRpm)}</strong>
                          <small>
                            {formatFanSpeedMeta(
                              displayedCpuFanRpm,
                              displayedCpuFanTargetPercent,
                            )}
                          </small>
                        </div>
                      </div>
                    </div>

                    <div className="fan-mode__footer">
                      <strong>{currentFanProfile.name}</strong>
                      <p>{currentFanProfile.summary}</p>
                      <button className="button" onClick={cloneToCustom}>
                        Clone to Custom
                      </button>
                    </div>
                  </>
                )}
              </div>
            </section>
            </div>
          )}

          {activeTab === 'power' && (
            <div
              className="page-stage-fit"
              style={{
                height: powerScaledHeight == null ? undefined : `${powerScaledHeight}px`,
              }}
            >
            <section
              className="panel panel--wide power-mode page-stage"
              ref={powerStageRef}
              style={{
                transform: powerScale === 1 ? undefined : `scale(${powerScale})`,
              }}
            >
              <div className="power-mode__toolbar">
                <div className="power-mode__field">
                  <span className="eyebrow">Power Profile</span>
                  <div className="power-mode__select">
                    <strong>{currentPowerProfile.name}</strong>
                    <small>{currentPowerProfile.strap}</small>
                  </div>
                </div>

                <div className="power-mode__field power-mode__field--meta">
                  <span className="eyebrow">Power Manager</span>
                  <div className="power-mode__meta-card" title={currentPowerManagerDetail}>
                    <div className="power-mode__meter-grid">
                      <div>
                        <span>CPU</span>
                        <strong>{cpuPowerReadoutValue}</strong>
                        <small>{cpuPowerMeterDetail}</small>
                      </div>
                      <div>
                        <span>GPU</span>
                        <strong>{gpuPowerReadoutValue}</strong>
                        <small>{gpuPowerMeterDetail}</small>
                      </div>
                    </div>
                  </div>
                </div>
              </div>

              <div className="power-mode__tabs">
                <button className="is-active">Mode</button>
              </div>

              <LayerStatusStrip
                statuses={powerLayerStatuses}
                emptyLabel="No power apply has run yet."
              />

              <div className="power-mode__frame">
                <div className="power-mode__header">
                  <span className="eyebrow">{formatPowerSource(displayedAcPluggedIn)}</span>
                </div>

                <div className="power-mode__profiles">
                  {powerProfiles.map((profile) => (
                    <button
                      key={profile.id}
                      className={`power-tile ${
                        activePowerProfile === profile.id ? 'is-selected' : ''
                      } ${glowTarget === profile.id ? 'is-pulsing' : ''}`}
                      onClick={() => handlePowerProfile(profile.id)}
                    >
                      <div className="power-tile__icon" />
                      <strong>{profile.name}</strong>
                      <small>{profile.strap}</small>
                    </button>
                  ))}
                </div>

                <div className="power-mode__dashboard">
                  <div className="power-gauge-card">
                    <div className="power-gauge-card__ring power-gauge-card__ring--gpu" />
                    <div className="power-gauge-card__content">
                      <span className="eyebrow">GPU</span>
                      <strong>{formatTelemetryValue(displayedGpuClock)}</strong>
                      <small>{displayedGpuClock == null ? '' : 'MHz'}</small>
                      <p>
                        {buildGpuPowerDashboardSummary(
                          displayedGpuTemp,
                          displayedGpuUsage,
                          displayedGpuPowerDraw,
                          displayedGpuPowerLimit,
                        )}
                      </p>
                    </div>
                  </div>

                  <div className="power-gauge-card">
                    <div className="power-gauge-card__ring power-gauge-card__ring--cpu" />
                    <div className="power-gauge-card__content">
                      <span className="eyebrow">CPU</span>
                      <strong>{formatTelemetryValue(displayedCpuClock)}</strong>
                      <small>{displayedCpuClock == null ? '' : 'MHz'}</small>
                      <p>
                        {buildCpuPowerDashboardSummary(
                          displayedCpuTemp,
                          displayedCpuUsage,
                          displayedCpuPackagePower,
                          displayedCpuPl1,
                          displayedCpuPl2,
                        )}
                      </p>
                      <p>
                        {buildCpuThermalSummary(
                          displayedCpuTempLowest,
                          displayedCpuTempHighest,
                          null,
                        )}
                      </p>
                    </div>
                  </div>
                </div>

                {activePowerProfile === 'custom' && (
                  <div className="power-custom-panel">
                    <div className="power-custom-panel__header">
                      <strong>Custom Processor State</strong>
                      <span>
                        Adjust how low the CPU can idle and how high it is allowed to boost.
                      </span>
                    </div>

                    <div className="power-setting power-setting--select">
                      <div className="power-setting__header power-setting__header--stacked">
                        <div className="power-setting__title power-setting__title--stacked">
                          <strong>Custom Firmware Base</strong>
                          <span>
                            Choose which Acer preset Custom should ride on before the processor
                            policy is layered on top.
                          </span>
                        </div>
                        <small>{currentCustomPowerBase.summary}</small>
                      </div>

                      <label className="power-select">
                        <span className="eyebrow">Base preset</span>
                        <select
                          value={customPowerBase}
                          onChange={(event) =>
                            updateCustomPowerBase(event.target.value as CustomPowerBaseId)
                          }
                        >
                          {customPowerBaseOptions.map((option) => (
                            <option key={option.id} value={option.id}>
                              {option.name}
                            </option>
                          ))}
                        </select>
                      </label>
                    </div>

                    <div className="power-custom-grid">
                      <ProcessorStateControl
                        label="Minimum Processor State"
                        min={0}
                        value={customProcessorState.min}
                        onChange={(value) => updateCustomProcessorState('min', value)}
                      />
                      <ProcessorStateControl
                        label="Maximum Processor State"
                        value={customProcessorState.max}
                        onChange={(value) => updateCustomProcessorState('max', value)}
                      />
                    </div>

                    <div className="power-oc-panel">
                      <div className="power-oc-panel__header">
                        <strong>GPU Overclocking</strong>
                        <span>
                          Core and memory offsets apply live through the NVIDIA path on this GPU.
                        </span>
                      </div>

                      <div className="power-oc-topline">
                        <div className="power-oc-brand">
                          <div className="power-oc-brand__mark">
                            <img src={aeroforgeMark} alt="AeroForge tuner mark" />
                          </div>
                          <div>
                            <span className="eyebrow">Tuner Deck</span>
                            <strong>{currentOcSlot.name}</strong>
                            <small>{currentOcSlot.strap}</small>
                          </div>
                        </div>

                        <div className="power-oc-state-grid">
                          <div className="power-oc-state-card">
                            <span className="eyebrow">Apply State</span>
                            <strong>{ocApplyState === 'live' ? 'Applied Live' : 'Staged'}</strong>
                            <small>
                              {ocApplyState === 'live'
                                ? 'Core and memory offsets are live. Voltage, power, and temp stay staged only.'
                                : 'Changes are waiting for an Apply action.'}
                            </small>
                          </div>
                          <div className="power-oc-state-card">
                            <span className="eyebrow">Lock State</span>
                            <strong>{ocTuningLocked ? 'Locked' : 'Unlocked'}</strong>
                            <small>
                              {ocTuningLocked
                                ? 'Sliders are frozen until the tuner is unlocked.'
                                : 'Controls are ready for live adjustments.'}
                            </small>
                          </div>
                        </div>
                      </div>

                      <div className="power-oc-slots">
                        {ocProfileSlots.map((slot) => (
                          <button
                            key={slot.id}
                            className={`power-oc-slot ${
                              activeOcSlot === slot.id ? 'is-selected' : ''
                            } ${glowTarget === slot.id ? 'is-pulsing' : ''}`}
                            onClick={() => handleOcProfileSlot(slot.id)}
                          >
                            <span>{slot.label}</span>
                            <strong>{slot.name}</strong>
                            <small>{slot.strap}</small>
                            {slot.isCustom && <em>Custom</em>}
                          </button>
                        ))}
                      </div>

                      <div className="power-oc-grid">
                        <OverclockSlider
                          label="Core Clock"
                          unit="MHz"
                          value={gpuOverclock.coreClock}
                          min={-250}
                          max={250}
                          step={5}
                          disabled={ocTuningLocked}
                          onChange={(value) => updateGpuOverclockSetting('coreClock', value)}
                        />
                        <OverclockSlider
                          label="Memory Clock"
                          unit="MHz"
                          value={gpuOverclock.memoryClock}
                          min={-1000}
                          max={1500}
                          step={10}
                          disabled={ocTuningLocked}
                          onChange={(value) => updateGpuOverclockSetting('memoryClock', value)}
                        />
                      </div>

                      <div className="power-oc-actions">
                        <button
                          className={`button button--primary ${
                            glowTarget === 'oc-apply' ? 'is-pulsing' : ''
                          }`}
                          onClick={handleApplyGpuTuning}
                        >
                          Apply Live
                        </button>
                        <button
                          className="button"
                          onClick={handleSaveGpuTuning}
                        >
                          Save To Custom
                        </button>
                        <button
                          className="button"
                          onClick={handleToggleOcLock}
                        >
                          {ocTuningLocked ? 'Unlock Tuner' : 'Lock Tuner'}
                        </button>
                        <button
                          className="button button--ghost"
                          onClick={handleResetGpuTuning}
                        >
                          Reset
                        </button>
                      </div>
                    </div>
                  </div>
                )}

                <div className="power-mode__footer">
                  <strong>{currentPowerProfile.name}</strong>
                  <p>{currentPowerSummary}</p>
                </div>
              </div>
            </section>
            </div>
          )}

          {activeTab === 'personal' && (
            <section className="panel panel--wide personal-mode">
              <div className="personal-mode__layout">
                <aside className="personal-sidebar">
                  <span className="eyebrow">Settings</span>

                  <div className="personal-sidebar__menu">
                    {personalSections.map((section) => (
                      <button
                        key={section.id}
                        className={`personal-sidebar__item ${
                          activePersonalSection === section.id ? 'is-active' : ''
                        }`}
                        onClick={() => setActivePersonalSection(section.id)}
                      >
                        <strong>{section.label}</strong>
                        <small>{section.description}</small>
                      </button>
                    ))}
                  </div>
                </aside>

                <div className="personal-detail">
                  {activePersonalSection === 'updates' ? (
                    <section className="personal-frame">
                      <div className="personal-frame__header">
                        <span className="eyebrow">Updates</span>
                      </div>

                      <div className="personal-frame__body">
                        <div className="settings-summary-grid">
                          <div className="settings-summary-card">
                            <span className="eyebrow">Current Version</span>
                            <strong>v{backendVersion}</strong>
                            <small>Desktop release currently installed on this machine.</small>
                          </div>

                          <div className="settings-summary-card">
                            <span className="eyebrow">Latest Available</span>
                            <strong>{updateLatestLabel}</strong>
                            <small>{updateStatus?.latestTitle ?? 'Checks use the published GitHub release feed.'}</small>
                          </div>

                          <div className="settings-summary-card">
                            <span className="eyebrow">Feed Status</span>
                            <strong>{updateActionLabel}</strong>
                            <small>
                              {updateActionPending
                                ? 'Working now. Buttons unlock when the action finishes.'
                                : `Last checked ${updateLastCheckedLabel}.`}
                            </small>
                          </div>
                        </div>

                        <div className="personal-setting-block">
                          <div>
                            <strong>Check for Updates on Launch</strong>
                            <p>
                              When enabled, AeroForge runs one published-release check after launch.
                            </p>
                          </div>

                          <button
                            className={`toggle ${checkForUpdatesOnLaunch ? 'is-on' : ''}`}
                            onClick={handleToggleUpdateChecksOnLaunch}
                            aria-pressed={checkForUpdatesOnLaunch}
                          >
                            <span />
                          </button>
                        </div>

                        <div
                          className={`settings-note ${
                            updateActionPending
                              ? 'settings-note--active'
                              : updateStatus?.lastError
                                ? 'settings-note--error'
                                : ''
                          }`}
                          aria-busy={updateActionPending !== null}
                        >
                          <div>
                            <span className="eyebrow">Release Status</span>
                            <strong>{updateActionLabel}</strong>
                            <p>{updateStatusDetail}</p>
                            {updateActionPending && (
                              <small className="settings-note__activity">
                                Action in progress. Leave AeroForge open until this finishes.
                              </small>
                            )}
                            {updateStatus?.stagedAssetName && (
                              <small>
                                {updateStatus.canInstallUpdate
                                  ? 'Staged package '
                                  : 'Previous staged package '}
                                {updateStatus.stagedAssetName}
                                {updateStatus.stagedSha256
                                  ? ` - SHA256 ${updateStatus.stagedSha256.slice(0, 16)}...${
                                      updateStatus.stagedSha256Verified
                                        ? ' verified.'
                                        : ' not verified by release metadata.'
                                    }`
                                  : updateStatus.canInstallUpdate
                                    ? ' - ready for install.'
                                    : ' - not installable for the current release.'}
                              </small>
                            )}
                            {updateStatus?.lastError && <small>Last error: {updateStatus.lastError}</small>}
                          </div>
                        </div>

                        <div className="settings-action-row">
                          <button
                            className="button"
                            disabled={updateActionPending !== null}
                            onClick={() => void runUpdateCheck(true)}
                            type="button"
                          >
                            {updateCheckButtonLabel}
                          </button>
                          <button
                            className="button"
                            disabled={!updateStatus?.canStageUpdate || updateActionPending !== null}
                            onClick={() => void handleStageLatestUpdate()}
                            type="button"
                          >
                            {updateDownloadButtonLabel}
                          </button>
                          <button
                            className="button button--ghost"
                            disabled={!updateStatus?.canInstallUpdate || updateActionPending !== null}
                            onClick={() => void handleInstallLatestUpdate()}
                            type="button"
                          >
                            {updateInstallButtonLabel}
                          </button>
                        </div>
                      </div>
                    </section>
                  ) : activePersonalSection === 'charge' ? (
                    <section className="personal-frame">
                      <div className="personal-frame__header">
                        <span className="eyebrow">Battery & Charge</span>
                      </div>

                      <div className="personal-frame__body">
                        <div className="charge-mode-grid">
                          <button
                            className={`charge-mode-card ${
                              smartChargingEnabled ? 'is-selected' : ''
                            }`}
                            disabled={settingsActionPending !== null || !smartChargeWritable}
                            onClick={() => void handleSmartChargingMode(true)}
                            type="button"
                          >
                            <div className="charge-mode-card__badge">80</div>
                            <div>
                              <strong>Optimized Battery Charging</strong>
                              <p>
                                {smartChargePending && smartChargingEnabled
                                  ? 'Applying the 80% battery-health ceiling now.'
                                  : 'Recommended for battery longevity. Charging is capped at 80%.'}
                              </p>
                            </div>
                          </button>

                          <button
                            className={`charge-mode-card ${
                              !smartChargingEnabled ? 'is-selected' : ''
                            }`}
                            disabled={settingsActionPending !== null || !smartChargeWritable}
                            onClick={() => void handleSmartChargingMode(false)}
                            type="button"
                          >
                            <div className="charge-mode-card__badge charge-mode-card__badge--full">
                              100
                            </div>
                            <div>
                              <strong>Full Battery Charging</strong>
                              <p>
                                {smartChargePending && !smartChargingEnabled
                                  ? 'Removing the Acer battery-health cap now.'
                                  : 'Allows maximum unplugged runtime by charging to full capacity.'}
                              </p>
                            </div>
                          </button>
                        </div>

                        {smartChargeDisabledReason && (
                          <div className="settings-note">
                            <div>
                              <span className="eyebrow">Charge Control Unavailable</span>
                              <strong>Battery-health switching is disabled</strong>
                              <p>{smartChargeDisabledReason}</p>
                            </div>
                          </div>
                        )}

                        <div className="personal-setting-block">
                          <div>
                            <strong>CPU Min/Max Writes</strong>
                            <p>
                              {processorStateControlEnabled
                                ? 'AeroForge can change Windows processor minimum and maximum state when applying power profiles.'
                                : 'Firmware power modes still apply; Windows processor minimum and maximum state is left unchanged.'}
                            </p>
                          </div>

                          <button
                            className={`toggle ${processorStateControlEnabled ? 'is-on' : ''}`}
                            onClick={() => void handleProcessorStateControlToggle()}
                            aria-pressed={processorStateControlEnabled}
                            type="button"
                          >
                            <span />
                          </button>
                        </div>

                        <div className="personal-setting-block">
                          <div>
                            <strong>NVIDIA Telemetry Polling</strong>
                            <p>
                              {nvidiaTelemetryPending
                                ? 'Applying the telemetry polling rule now.'
                                : nvidiaTelemetryEnabled
                                  ? 'AeroForge may read NVIDIA clocks, power, limits, and apply Whisper hints when Windows reports active dGPU work.'
                                  : 'AeroForge skips NVIDIA polling and Whisper hints so the dGPU can stay idle; firmware fan and temperature readings remain available.'}
                            </p>
                          </div>

                          <button
                            className={`toggle ${nvidiaTelemetryEnabled ? 'is-on' : ''}`}
                            disabled={settingsActionPending !== null}
                            onClick={() => void handleNvidiaTelemetryToggle()}
                            aria-pressed={nvidiaTelemetryEnabled}
                            type="button"
                          >
                            <span />
                          </button>
                        </div>

                        {usbPowerVisible && (
                          <div className="personal-setting-block">
                            <div>
                              <strong>Power-off USB Charger</strong>
                              <p>
                                Keep the designated USB port powered for accessories even when the
                                laptop is sleeping or shut down.
                              </p>
                              {usbPowerDisabledReason && <small>{usbPowerDisabledReason}</small>}
                            </div>

                            <button
                              className={`toggle ${usbPowerEnabled ? 'is-on' : ''}`}
                              disabled={!usbPowerWritable}
                              onClick={() => {
                                if (usbPowerDisabledReason) {
                                  setStatusMessage(usbPowerDisabledReason)
                                  return
                                }

                                setUsbPowerEnabled((current) => !current)
                                setStatusMessage(
                                  usbPowerEnabled
                                    ? 'Power-off USB charging disabled in the preview.'
                                    : 'Power-off USB charging enabled in the preview.',
                                )
                              }}
                              aria-pressed={usbPowerEnabled}
                              type="button"
                            >
                              <span />
                            </button>
                          </div>
                        )}

                        <div className="personal-setting-block">
                          <div>
                            <strong>Auto 60 Hz on Battery</strong>
                            <p>
                              {refreshRatePending
                                ? 'Applying the display refresh-rate rule now.'
                                : autoRefreshRateOnBatteryEnabled
                                  ? autoRefreshRateRestoreHz
                                    ? `Battery mode will use 60 Hz, then restore ${autoRefreshRateRestoreHz} Hz on AC power.`
                                    : 'Battery mode will use 60 Hz when AeroForge detects unplugged power.'
                                  : 'Switches the display to 60 Hz on battery and restores the previous refresh rate on AC power.'}
                            </p>
                          </div>

                          <button
                            className={`toggle ${autoRefreshRateOnBatteryEnabled ? 'is-on' : ''}`}
                            disabled={settingsActionPending !== null}
                            onClick={() => void handleAutoRefreshRateToggle()}
                            aria-pressed={autoRefreshRateOnBatteryEnabled}
                            type="button"
                          >
                            <span />
                          </button>
                        </div>

                        <div className="personal-setting-block">
                          <div>
                            <strong>Charge Limit While Plugged In</strong>
                            <p>
                              Uses Acer battery-health control directly when available. Optimized
                              mode keeps the 80% ceiling, while full mode clears the cap and allows
                              charging to 100%.
                            </p>
                          </div>

                          <div className="charge-limit-chip">{smartChargeTarget}</div>
                        </div>

                        <div className="battery-meter personal-battery-meter">
                          <div
                            className="battery-meter__fill"
                            style={{ width: smartChargingEnabled ? '80%' : '100%' }}
                          />
                        </div>

                        <div className="battery-meter__labels">
                          <span>Wear-aware target</span>
                          <strong>{smartChargeTarget}</strong>
                        </div>
                      </div>
                    </section>
                  ) : activePersonalSection === 'screen' ? (
                    <section className="personal-frame">
                      <div className="personal-frame__header">
                        <span className="eyebrow">Screen</span>
                      </div>

                      <div className="personal-frame__body">
                        <div className="personal-setting-block">
                          <div>
                            <strong>Blue Light Filter</strong>
                            <p>
                              Applies the Acer-style eye-care gamma ramp directly to the display for
                              lower blue output during long sessions.
                            </p>
                            {blueLightDisabledReason && <small>{blueLightDisabledReason}</small>}
                          </div>

                          <button
                            className={`toggle ${blueLightFilterEnabled ? 'is-on' : ''}`}
                            disabled={settingsActionPending !== null || !blueLightWritable}
                            onClick={() => void handleBlueLightFilterToggle()}
                            aria-pressed={blueLightFilterEnabled}
                            type="button"
                          >
                            <span />
                          </button>
                        </div>

                        <div
                          className={`screen-preview ${
                            blueLightFilterEnabled ? 'is-filtered' : ''
                          }`}
                        >
                          <div className="screen-preview__panel">
                            <span className="eyebrow">Panel Profile</span>
                            <strong>
                              {blueLightFilterEnabled
                                ? blueLightPending
                                  ? 'Applying warm Acer eye-care profile'
                                  : 'Warm Acer eye-care profile'
                                : 'Neutral panel profile'}
                            </strong>
                            <p>
                              {blueLightFilterEnabled
                                ? 'Gamma ramp shifted toward Acer GainID 3 warmth for lower blue output.'
                                : 'Standard color balance with no comfort filter applied.'}
                            </p>
                          </div>
                        </div>
                      </div>
                    </section>
                  ) : activePersonalSection === 'boot' ? (
                    <section className="personal-frame">
                      <div className="personal-frame__header">
                        <span className="eyebrow">System Boot Effect</span>
                      </div>

                      <div className="personal-frame__body personal-frame__body--boot">
                          <div className="boot-setting-copy">
                            <strong>Boot Logo Customization</strong>
                            <p>
                              {bootLogoWritable
                                ? 'Click a built-in AeroForge splash to apply it, or upload an image. AeroForge preserves GIF files and converts static images to firmware-safe JPEG before apply.'
                                : bootLogoStatusText}
                            </p>
                            <p className="boot-logo-notice">
                              Boot logo changing is currently non-functional. If you want this
                              feature to become reliable, consider contributing to the project.
                            </p>
                          </div>

                        <div className="boot-preview-panel">
                          <div className="boot-preview-panel__canvas">
                            {selectedBootArt === 'custom' && customBootPreview ? (
                              <img
                                className="branding-preview__image"
                                src={customBootPreview}
                                alt="Custom boot splash preview"
                              />
                            ) : (
                              currentBootArt && <BootSplashPreview art={currentBootArt} />
                            )}
                          </div>

                          <div className="boot-preview-panel__footer boot-preview-panel__footer--stacked">
                            <span className="eyebrow">Current Boot Logo</span>
                            <strong>
                              {selectedBootArt === 'custom'
                                ? customBootFilename
                                : currentBootArt?.name ?? 'Preset boot image'}
                            </strong>
                            <small>
                              {bootLogoWritable
                                ? selectedBootArt === 'custom'
                                  ? bootLogoPending
                                    ? 'Applying custom splash through AeroForge'
                                    : 'Custom splash applied through AeroForge'
                                  : bootLogoPending
                                    ? 'Applying bundled AeroForge splash through AeroForge'
                                    : 'Click any AeroForge preset tile below to write it as the firmware splash.'
                                : bootLogoStatusText}
                            </small>
                          </div>
                        </div>

                        <div className="branding-controls branding-controls--boot">
                          <label
                            className={`upload-card upload-card--boot ${
                              !bootLogoWritable || settingsActionPending !== null ? 'is-disabled ' : ''
                            }${
                              glowTarget === 'boot-upload' ? 'is-pulsing' : ''
                            }`}
                            aria-disabled={!bootLogoWritable || settingsActionPending !== null}
                          >
                            <input
                              type="file"
                              accept="image/*"
                              onChange={handleBootFile}
                              disabled={!bootLogoWritable || settingsActionPending !== null}
                            />
                            <strong>{bootLogoPending ? 'Applying splash' : 'Upload custom splash'}</strong>
                            <span>
                              {bootLogoWritable
                                ? 'PNG, JPG, WEBP, or GIF. GIF stays GIF; static images become JPG.'
                                : bootLogoStatusText}
                            </span>
                          </label>

                          <div className="boot-presets">
                            <div className="boot-presets__header">
                              <span className="eyebrow">Boot Logo Previews</span>
                            </div>

                            <div className="art-grid art-grid--boot">
                              {bootArtwork.map((art) => (
                                <button
                                  type="button"
                                  key={art.id}
                                  className={`art-tile ${
                                    selectedBootArt === art.id ? 'is-selected' : ''
                                  }`}
                                  disabled={settingsActionPending !== null}
                                  onClick={() => void handleBootArtworkApply(art)}
                                >
                                  <div className="art-tile__swatch art-tile__swatch--boot">
                                    <BootSplashPreview art={art} compact />
                                  </div>
                                  <strong>{art.name}</strong>
                                </button>
                              ))}
                            </div>
                          </div>
                        </div>
                      </div>
                    </section>
                  ) : activePersonalSection === 'backup' ? (
                    <section className="personal-frame">
                      <div className="personal-frame__header">
                        <span className="eyebrow">Backup & Restore</span>
                      </div>

                      <div className="personal-frame__body">
                        <div className="settings-note">
                          <div>
                            <span className="eyebrow">Portable settings</span>
                            <strong>Export your full AeroForge control profile</strong>
                            <p>
                              Saves power and fan modes, custom curves, presets, GPU tuning slots,
                              and personal toggles as JSON. Does not include update tokens or service
                              secrets. After import, re-apply power/fan modes to push them live to
                              hardware.
                            </p>
                          </div>
                        </div>

                        <div className="personal-setting-block">
                          <div>
                            <strong>Export settings backup</strong>
                            <p>
                              Download a portable JSON snapshot of the current staged desktop state.
                            </p>
                          </div>
                          <button
                            className="button"
                            type="button"
                            onClick={exportSettingsBackup}
                          >
                            Export Backup
                          </button>
                        </div>

                        <div className="personal-setting-block">
                          <div>
                            <strong>Import settings backup</strong>
                            <p>
                              Replace local staged settings from a previously exported AeroForge
                              settings file.
                            </p>
                          </div>
                          <button
                            className="button button--ghost"
                            type="button"
                            onClick={triggerSettingsImport}
                          >
                            Import Backup
                          </button>
                        </div>

                        <input
                          ref={settingsImportInputRef}
                          type="file"
                          accept="application/json,.json"
                          className="visually-hidden"
                          onChange={(event) => void handleSettingsImportFile(event)}
                          aria-hidden
                          tabIndex={-1}
                        />
                      </div>
                    </section>
                  ) : null}
                </div>
              </div>
            </section>
          )}

          {activeTab === 'debug' && (
            <section className="panel panel--wide debug-workspace">
              <div className="section-heading">
                <div>
                  <span className="eyebrow">Debug</span>
                  <h2>Backend Diagnostics</h2>
                </div>
                <div className="debug-workspace__actions">
                  <button
                    type="button"
                    className="button button--ghost"
                    onClick={() => void copyDiagnosticsToClipboard()}
                  >
                    Copy Report
                  </button>
                  <button
                    type="button"
                    className="button button--ghost"
                    onClick={exportDiagnosticsFile}
                  >
                    Export JSON
                  </button>
                  <button type="button" className="button button--ghost" onClick={clearDebugEvents}>
                    Clear Log
                  </button>
                </div>
              </div>
              <p className="debug-shortcut-hint">{KEYBOARD_SHORTCUT_HINT}</p>

              <div className="debug-status-grid">
                <div className="debug-card">
                  <span>Runtime Shell</span>
                  <strong>{shellStatus}</strong>
                  <small>
                    {serviceConnected
                      ? 'Desktop shell connected.'
                      : 'Using browser or local fallback shell path.'}
                  </small>
                </div>

                <div className="debug-card">
                  <span>Preview Status</span>
                  <strong>{statusMessage}</strong>
                  <small>
                    Active combination:{' '}
                    {powerProfiles.find((profile) => profile.id === activePowerProfile)?.name}
                    {' / '}
                    {fanProfiles.find((profile) => profile.id === activeFanProfile)?.name}
                  </small>
                </div>

                <div className="debug-card">
                  <span>Firmware Path</span>
                  <strong>{serviceConnected ? 'Service gated' : 'Disabled'}</strong>
                  <small>
                    Fan writes use direct AcerGamingFunction WMI/ACPI calls with telemetry verification.
                  </small>
                </div>
              </div>

              <section className="debug-panel debug-panel--standalone">
                <div className="debug-panel__header">
                  <span className="eyebrow">Backend Debug</span>
                  <strong>
                    {serviceConnected ? 'Named pipe reachable' : 'Named pipe unavailable'}
                  </strong>
                </div>

                <div className="debug-grid">
                  <div className="debug-card">
                    <span>Transport</span>
                    <strong>{telemetrySourceLabel}</strong>
                    <small>
                      {serviceStatus?.detail ?? lastBackendError ?? 'No service detail yet.'}
                    </small>
                  </div>

                  <div className="debug-card">
                    <span>Last Poll</span>
                    <strong>{lastBackendPollAt}</strong>
                    <small>
                      Supervisor updated {formatUnixClock(serviceStatus?.updatedAtUnix)} with{' '}
                      {serviceStatus?.workerCount ?? 0} workers reported.
                    </small>
                  </div>

                  <div className="debug-card">
                    <span>Snapshot</span>
                    <strong>
                      {activeTelemetry
                        ? `CPU ${
                            presentPositive(
                              activeTelemetry.cpuTempAverageC ?? activeTelemetry.cpuTempC ?? null,
                            ) ?? '?'
                          }C / GPU ${presentPositive(activeTelemetry.gpuTempC ?? null) ?? '?'}C`
                        : 'No live telemetry'}
                    </strong>
                    <small>
                      {activeTelemetry
                        ? `CPU fan ${activeTelemetry.cpuFanRpm} RPM, GPU fan ${activeTelemetry.gpuFanRpm} RPM, CPU power ${
                            formatWattValue(activeTelemetry.cpuPackagePowerW, 1) ?? '?'
                          }, GPU power ${
                            formatWattValue(activeTelemetry.gpuPowerDrawW, 1) ?? '?'
                          }, battery ${activeTelemetry.batteryPercent}%.`
                        : ''}
                    </small>
                  </div>

                  <div className="debug-card">
                    <span>Frame Time</span>
                    <strong>{formatFrameTime(frameStats.averageMs) || 'Waiting'}</strong>
                    <small>
                      Max {formatFrameTime(frameStats.maxMs) || 'Waiting'},{' '}
                      {Math.round(frameStats.fps) || 0} fps, {frameStats.longFrameCount} long
                      frames. Sampled at {frameStats.updatedAt}.
                    </small>
                  </div>

                  <div className="debug-card">
                    <span>Stage Fit</span>
                    <strong>{formatStageFitValue(activeStageFitSnapshot) ?? 'Waiting'}</strong>
                    <small>{activeStageFitDetail ?? stageFitDetail}</small>
                  </div>

                  <div className="debug-card">
                    <span>Perf Log</span>
                    <strong>
                      {performanceLogState.path
                        ? performanceLogState.path.split(/[\\/]/).pop()
                        : isDesktopRuntime()
                          ? 'Waiting'
                          : 'Browser only'}
                    </strong>
                    <small>
                      {performanceLogState.lastError
                        ? `Write failed: ${performanceLogState.lastError}`
                        : `${performanceLogState.eventCount} events, ${performanceLogState.pendingCount} pending. Last flush ${performanceLogState.lastFlushAt}.`}
                    </small>
                  </div>
                </div>

                <div className="debug-log">
                  <div className="debug-log__header">
                    <span className="eyebrow">Recent Events</span>
                    <small>
                      {lastBackendError
                        ? `Last error: ${lastBackendError}`
                        : 'No invoke errors captured.'}
                    </small>
                  </div>

                  <ul>
                    {debugEvents.length === 0 ? (
                      <li>No recent debug events yet. Switch profiles or wait for a backend poll.</li>
                    ) : (
                      debugEvents.map((event) => <li key={event}>{event}</li>)
                    )}
                  </ul>
                </div>
              </section>
            </section>
          )}
        </div>
      </main>
      <ToastStack toasts={toasts} onDismiss={dismissToast} />
      <ShortcutsOverlay
        open={shortcutsHelpOpen}
        entries={KEYBOARD_SHORTCUT_ENTRIES}
        onClose={() => setShortcutsHelpOpen(false)}
      />
    </div>
  )
}

export default App

