import {
  type ChangeEvent,
  useEffect,
  useRef,
  useState,
} from 'react'
import { getCurrentWindow } from '@tauri-apps/api/window'
import './App.css'
import aeroforgeMark from './assets/aeroforge-mark.png'
import aeroforgeWordmark from './assets/aeroforge-wordmark.png'
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
  getBackendPollSnapshot,
  getLiveControlSnapshot,
  installStagedUpdate,
  saveControlSnapshot,
  setNvidiaTelemetryEnabled,
  showUpdateNotification,
  stageUpdateDownload,
  type CapabilitySnapshot,
  type ControlSnapshot,
  type BootArtId,
  type CustomPowerBaseId,
  type FeatureSupport,
  type LiveControlSnapshot,
  type PerformanceLogEvent,
  type ServiceStatus,
  type TelemetrySnapshot,
  type UpdateStatus,
} from './lib/backend'

type CurveTarget = 'cpu' | 'gpu'
type ControlTab = 'home' | 'power' | 'fans' | 'personal' | 'debug'
type PersonalSection = 'updates' | 'charge' | 'screen' | 'boot'
type UpdateChannel = ControlSnapshot['personalSettings']['updateChannel']
type UpdateAction = 'check' | 'stage' | 'install'
type CurvePoint = {
  temp: number
  speed: number
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

type CurveSet = Record<CurveTarget, CurvePoint[]>

type PowerProfile = {
  id: 'battery-guard' | 'balanced' | 'performance' | 'turbo' | 'custom'
  name: string
  strap: string
  summary: string
  wattage: string
  runtime: string
}

type FanProfile = {
  id: 'auto' | 'max' | 'custom'
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

type BootArt = {
  id: string
  name: string
  palette: string
  layout: 'forge' | 'center' | 'banner'
  headline: string
  subline: string
}

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

const bootArtwork: BootArt[] = [
  {
    id: 'ember',
    name: 'Forge Ember',
    palette: 'palette-ember',
    layout: 'forge',
    headline: 'AeroForge',
    subline: 'Thermal performance boot',
  },
  {
    id: 'arc',
    name: 'Arc Horizon',
    palette: 'palette-arc',
    layout: 'banner',
    headline: 'AeroForge Arc',
    subline: 'Cool-spectrum startup theme',
  },
  {
    id: 'slate',
    name: 'Slate Monolith',
    palette: 'palette-slate',
    layout: 'center',
    headline: 'AF Core',
    subline: 'Minimal studio boot screen',
  },
]

const defaultGpuOverclock: GpuTuningState = {
  coreClock: 165,
  memoryClock: 420,
  voltageOffset: -35,
  powerLimit: 114,
  tempLimit: 83,
}

const builtInOcProfileSlots: OcProfileSlot[] = [
  {
    id: 'silent-uv',
    label: 'P1',
    name: 'Silent UV',
    strap: 'Low-noise undervolt',
    settings: {
      coreClock: 90,
      memoryClock: 180,
      voltageOffset: -60,
      powerLimit: 92,
      tempLimit: 78,
    },
  },
  {
    id: 'daily',
    label: 'P2',
    name: 'Forge Daily',
    strap: 'Balanced everyday tune',
    settings: {
      ...defaultGpuOverclock,
    },
  },
  {
    id: 'creator',
    label: 'P3',
    name: 'Creator Boost',
    strap: 'Long-session render preset',
    settings: {
      coreClock: 185,
      memoryClock: 560,
      voltageOffset: -10,
      powerLimit: 118,
      tempLimit: 84,
    },
  },
  {
    id: 'arena',
    label: 'P4',
    name: 'Arena Max',
    strap: 'Aggressive gaming tune',
    settings: {
      coreClock: 220,
      memoryClock: 840,
      voltageOffset: 25,
      powerLimit: 122,
      tempLimit: 86,
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
]

const fanTelemetryByProfile: Record<
  FanProfile['id'],
  { gpuRpm: number; cpuRpm: number; modeLabel: string }
> = {
  auto: { gpuRpm: 3385, cpuRpm: 3089, modeLabel: 'Automatic airflow mapping' },
  max: { gpuRpm: 5110, cpuRpm: 4950, modeLabel: 'Full-speed thermal override' },
  custom: { gpuRpm: 3385, cpuRpm: 3089, modeLabel: 'Custom curve runtime' },
}

const clamp = (value: number, min: number, max: number) =>
  Math.min(max, Math.max(min, value))

const chartWidth = 480
const chartHeight = 260
const chartPadding = 24
const tempMin = 30
const tempMax = 90
const speedMin = 0
const speedMax = 100
const BACKEND_POLL_INTERVAL_MS = 1000
const HIDDEN_BACKEND_POLL_INTERVAL_MS = 5000
const RUNTIME_ESTIMATE_COUNTDOWN_SEC = 30
const PERFORMANCE_LOG_BATCH_SIZE = 16
const PERFORMANCE_LOG_FLUSH_DELAY_MS = 1500
const PERFORMANCE_LOG_LONG_FRAME_MS = 34
const PERFORMANCE_LOG_MAX_QUEUE = 200
const FAN_PROFILE_APPLY_TIMEOUT_MS = 15_000

function pointToChart(point: CurvePoint) {
  const x =
    chartPadding +
    ((point.temp - tempMin) / (tempMax - tempMin)) * (chartWidth - chartPadding * 2)
  const y =
    chartHeight -
    chartPadding -
    ((point.speed - speedMin) / (speedMax - speedMin)) * (chartHeight - chartPadding * 2)

  return { x, y }
}

function buildCurvePath(points: CurvePoint[]) {
  return points
    .map((point, index) => {
      const { x, y } = pointToChart(point)
      return `${index === 0 ? 'M' : 'L'} ${x} ${y}`
    })
    .join(' ')
}

function normalizeCurvePoints(points: CurvePoint[]) {
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

function duplicateCurveSet(curves: CurveSet): CurveSet {
  return {
    cpu: normalizeCurvePoints(curves.cpu),
    gpu: normalizeCurvePoints(curves.gpu),
  }
}

function otherCurveTarget(target: CurveTarget): CurveTarget {
  return target === 'cpu' ? 'gpu' : 'cpu'
}

function mirrorCurveSetFromTarget(curves: CurveSet, source: CurveTarget): CurveSet {
  const normalized = duplicateCurveSet(curves)
  return {
    ...normalized,
    [otherCurveTarget(source)]: normalized[source].map((point) => ({ ...point })),
  }
}

function describeError(error: unknown) {
  if (error instanceof Error) {
    return error.message
  }

  if (typeof error === 'string') {
    return error
  }

  try {
    return JSON.stringify(error)
  } catch {
    return 'Unknown error'
  }
}

function withTimeout<T>(promise: Promise<T>, timeoutMs: number, label: string) {
  let timeoutId: number | null = null

  const timeout = new Promise<T>((_, reject) => {
    timeoutId = window.setTimeout(() => {
      reject(new Error(`${label} timed out after ${Math.round(timeoutMs / 1000)} seconds`))
    }, timeoutMs)
  })

  return Promise.race([promise, timeout]).finally(() => {
    if (timeoutId !== null) {
      window.clearTimeout(timeoutId)
    }
  })
}

type PreparedBootLogo = {
  fileName: string
  imageBase64: string
}

type BootArtworkTheme = {
  glowColor: string
  gradientStops: [string, string, string]
}

function readFileAsDataUrl(file: File) {
  return new Promise<string>((resolve, reject) => {
    const reader = new FileReader()
    reader.onload = () => resolve(String(reader.result ?? ''))
    reader.onerror = () => reject(reader.error ?? new Error('Unable to read boot-logo image.'))
    reader.readAsDataURL(file)
  })
}

function loadImageElement(src: string) {
  return new Promise<HTMLImageElement>((resolve, reject) => {
    const image = new Image()
    image.onload = () => resolve(image)
    image.onerror = () => reject(new Error('Unable to decode boot-logo image.'))
    image.src = src
  })
}

function buildJpegBootLogoName(fileName: string) {
  const stem = fileName.replace(/\.[^.]+$/, '').trim() || 'aeroforge-boot-logo'
  return `${stem}.jpg`
}

function buildGifBootLogoName(fileName: string) {
  const stem = fileName.replace(/\.[^.]+$/, '').trim() || 'aeroforge-boot-logo'
  return `${stem}.gif`
}

function base64PayloadFromDataUrl(dataUrl: string) {
  const payload = dataUrl.split(',')[1]
  if (!payload) {
    throw new Error('Unable to encode boot-logo image.')
  }
  return payload
}

function buildPresetBootLogoName(art: BootArt) {
  return `aeroforge-${art.id}-boot-logo.jpg`
}

function getBootArtworkTheme(art: BootArt): BootArtworkTheme {
  switch (art.id) {
    case 'arc':
      return {
        glowColor: 'rgba(133, 224, 255, 0.35)',
        gradientStops: ['#09141d', '#173d5a', '#4594bb'],
      }
    case 'slate':
      return {
        glowColor: 'rgba(255, 255, 255, 0.22)',
        gradientStops: ['#15181e', '#414a59', '#6c7585'],
      }
    case 'ember':
    default:
      return {
        glowColor: 'rgba(255, 170, 92, 0.34)',
        gradientStops: ['#30170f', '#9d4d2e', '#cb8a53'],
      }
  }
}

function drawBootArtworkBackground(
  context: CanvasRenderingContext2D,
  art: BootArt,
  width: number,
  height: number,
) {
  const theme = getBootArtworkTheme(art)
  const background = context.createLinearGradient(0, 0, width, height)
  background.addColorStop(0, theme.gradientStops[0])
  background.addColorStop(0.58, theme.gradientStops[1])
  background.addColorStop(1, theme.gradientStops[2])
  context.fillStyle = background
  context.fillRect(0, 0, width, height)

  const glow =
    art.layout === 'center'
      ? context.createRadialGradient(
          width * 0.5,
          height * 0.52,
          0,
          width * 0.5,
          height * 0.52,
          width * 0.34,
        )
      : context.createRadialGradient(
          width * 0.28,
          height * 0.28,
          0,
          width * 0.28,
          height * 0.28,
          width * 0.36,
        )
  glow.addColorStop(0, theme.glowColor)
  glow.addColorStop(1, 'rgba(0, 0, 0, 0)')
  context.fillStyle = glow
  context.fillRect(0, 0, width, height)
}

async function preparePresetBootLogo(art: BootArt): Promise<PreparedBootLogo> {
  const width = 1920
  const height = 1080
  const canvas = document.createElement('canvas')
  canvas.width = width
  canvas.height = height

  const context = canvas.getContext('2d')
  if (!context) {
    throw new Error('Unable to prepare boot-logo canvas.')
  }

  drawBootArtworkBackground(context, art, width, height)

  const [markImage, wordmarkImage] = await Promise.all([
    loadImageElement(aeroforgeMark),
    loadImageElement(aeroforgeWordmark),
  ])

  context.textBaseline = 'top'
  context.shadowColor = 'rgba(0, 0, 0, 0.34)'
  context.shadowBlur = 28
  context.shadowOffsetY = 12

  if (art.layout === 'center') {
    const markWidth = 340
    const markHeight = (markImage.naturalHeight / markImage.naturalWidth) * markWidth
    context.drawImage(markImage, (width - markWidth) / 2, 180, markWidth, markHeight)
    context.shadowBlur = 0
    context.shadowOffsetY = 0
    context.fillStyle = '#f6f1ea'
    context.font = '700 118px "Segoe UI", Arial, sans-serif'
    context.textAlign = 'center'
    context.fillText(art.headline, width / 2, 620)
    context.fillStyle = 'rgba(247, 239, 232, 0.82)'
    context.font = '500 40px "Segoe UI", Arial, sans-serif'
    context.fillText(art.subline.toUpperCase(), width / 2, 760)
  } else if (art.layout === 'banner') {
    const wordmarkWidth = 760
    const wordmarkHeight = (wordmarkImage.naturalHeight / wordmarkImage.naturalWidth) * wordmarkWidth
    context.drawImage(wordmarkImage, 170, 650, wordmarkWidth, wordmarkHeight)
    const markWidth = 270
    const markHeight = (markImage.naturalHeight / markImage.naturalWidth) * markWidth
    context.drawImage(markImage, 1430, 130, markWidth, markHeight)
    context.shadowBlur = 0
    context.shadowOffsetY = 0
    context.fillStyle = 'rgba(247, 239, 232, 0.82)'
    context.font = '500 38px "Segoe UI", Arial, sans-serif'
    context.textAlign = 'left'
    context.fillText(art.subline.toUpperCase(), 182, 845)
  } else {
    const markWidth = 280
    const markHeight = (markImage.naturalHeight / markImage.naturalWidth) * markWidth
    context.drawImage(markImage, 170, 140, markWidth, markHeight)
    context.shadowBlur = 0
    context.shadowOffsetY = 0
    context.drawImage(
      wordmarkImage,
      180,
      700,
      760,
      (wordmarkImage.naturalHeight / wordmarkImage.naturalWidth) * 760,
    )
    context.fillStyle = 'rgba(247, 239, 232, 0.82)'
    context.font = '500 38px "Segoe UI", Arial, sans-serif'
    context.textAlign = 'left'
    context.fillText(art.subline.toUpperCase(), 190, 830)
  }

  const jpegDataUrl = canvas.toDataURL('image/jpeg', 0.94)
  return {
    fileName: buildPresetBootLogoName(art),
    imageBase64: base64PayloadFromDataUrl(jpegDataUrl),
  }
}

async function prepareBootLogoUpload(file: File): Promise<PreparedBootLogo> {
  const sourceDataUrl = await readFileAsDataUrl(file)
  if (file.type === 'image/gif' || /\.gif$/i.test(file.name)) {
    return {
      fileName: buildGifBootLogoName(file.name),
      imageBase64: base64PayloadFromDataUrl(sourceDataUrl),
    }
  }

  const image = await loadImageElement(sourceDataUrl)
  const maxWidth = Math.max(1, Math.floor(window.screen.width * 0.4))
  const maxHeight = Math.max(1, Math.floor(window.screen.height * 0.4))
  const scale = Math.min(1, maxWidth / image.naturalWidth, maxHeight / image.naturalHeight)
  const width = Math.max(1, Math.round(image.naturalWidth * scale))
  const height = Math.max(1, Math.round(image.naturalHeight * scale))
  const canvas = document.createElement('canvas')
  canvas.width = width
  canvas.height = height

  const context = canvas.getContext('2d')
  if (!context) {
    throw new Error('Unable to prepare boot-logo canvas.')
  }

  context.fillStyle = '#000'
  context.fillRect(0, 0, width, height)
  context.drawImage(image, 0, 0, width, height)

  const jpegDataUrl = canvas.toDataURL('image/jpeg', 0.92)

  return {
    fileName: buildJpegBootLogoName(file.name),
    imageBase64: base64PayloadFromDataUrl(jpegDataUrl),
  }
}

function formatDebugClock(date: Date) {
  return date.toLocaleTimeString([], {
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
    hour12: false,
  })
}

function formatUnixClock(unix: number | null | undefined) {
  if (!unix) {
    return 'Waiting'
  }

  return formatDebugClock(new Date(unix * 1000))
}

function formatTelemetryValue(value: number | null | undefined, suffix = '') {
  if (value == null) {
    return ''
  }

  return `${value}${suffix}`
}

function formatFanSpeedMeta(
  rpm: number | null | undefined,
  targetPercent: number | null | undefined,
) {
  const rpmLabel = rpm == null ? null : 'RPM'
  const targetLabel = targetPercent == null ? null : `Target ${targetPercent}%`
  return [rpmLabel, targetLabel].filter(Boolean).join(' | ')
}

function buildFanSpeedDetail(
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

function formatWattValue(value: number | null | undefined, maximumFractionDigits = 0) {
  if (value == null || Number.isNaN(value)) {
    return null
  }

  return `${value.toLocaleString(undefined, { maximumFractionDigits })}W`
}

function formatWattReadout(value: number | null | undefined, maximumFractionDigits = 1) {
  return formatWattValue(value, maximumFractionDigits) ?? '--'
}

function buildCpuPowerLimitLabel(pl1Label: string | null, pl2Label: string | null) {
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

function buildGpuPowerLimitLabel(
  limitLabel: string | null,
  maxLimit: number | null | undefined,
) {
  if (limitLabel) {
    return `Limit ${limitLabel}`
  }

  const maxLimitLabel = formatWattValue(maxLimit, 1)
  return maxLimitLabel ? `Max ${maxLimitLabel}` : null
}

function sanitizeIdentityText(value: string | null | undefined) {
  if (!value) {
    return null
  }

  const sanitized = value
    .replace(/\(R\)|\(TM\)/gi, '')
    .replace(/\s+/g, ' ')
    .trim()

  return sanitized.length > 0 ? sanitized : null
}

function buildHardwareIdentity(
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

function formatLiveBatteryDetail(value: number | null | undefined) {
  if (value == null) {
    return ''
  }

  return `${value}% battery live`
}

function formatRemainingRuntime(seconds: number | null | undefined) {
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

function formatFrameTime(value: number) {
  if (!Number.isFinite(value) || value <= 0) {
    return ''
  }

  return `${value.toFixed(1)} ms`
}

function isDesktopRuntime() {
  return Boolean((window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__)
}

function waitForNextPaint() {
  return new Promise<void>((resolve) => {
    if (typeof window.requestAnimationFrame !== 'function') {
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

function presentPositive(value: number | null | undefined) {
  if (value == null || value <= 0) {
    return null
  }

  return value
}

function formatPowerSource(acPluggedIn: boolean | null | undefined) {
  if (acPluggedIn == null) {
    return 'Power source unavailable'
  }

  return acPluggedIn ? 'AC Plugged In' : 'Battery Power'
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
  setNvidiaTelemetryEnabled(controls.personalSettings.nvidiaTelemetryEnabled ?? true)
  setUsbPowerEnabled(controls.personalSettings.usbPowerEnabled)
  setBlueLightFilterEnabled(controls.personalSettings.blueLightFilterEnabled)
  setAutoRefreshRateSettings?.(
    controls.personalSettings.autoRefreshRateOnBatteryEnabled,
    controls.personalSettings.autoRefreshRateRestoreHz,
  )
  setSelectedBootArt(controls.personalSettings.selectedBootArt)
  setCustomBootFilename(controls.personalSettings.customBootFilename)
  setUpdateChannel('stable')
  setCheckForUpdatesOnLaunch(controls.personalSettings.checkForUpdatesOnLaunch)
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
  const runtimeEstimateSessionRef = useRef(false)
  const [activeTab, setActiveTab] = useState<ControlTab>('home')
  const [activePowerProfile, setActivePowerProfile] =
    useState<PowerProfile['id']>('turbo')
  const [customProcessorState, setCustomProcessorState] = useState({
    min: 35,
    max: 88,
  })
  const customProcessorStateRef = useRef(customProcessorState)
  const [customPowerBase, setCustomPowerBase] = useState<CustomPowerBaseId>('performance')
  const customPowerBaseRef = useRef<CustomPowerBaseId>('performance')
  const customPowerApplyTimerRef = useRef<number | null>(null)
  const customPowerApplyRevisionRef = useRef(0)
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
  const [fanSyncLockEnabled, setFanSyncLockEnabled] = useState(false)
  const [draggingPoint, setDraggingPoint] = useState<DragState | null>(null)
  const [smartChargingEnabled, setSmartChargingEnabled] = useState(true)
  const smartChargingEnabledRef = useRef(true)
  const [processorStateControlEnabled, setProcessorStateControlEnabled] = useState(true)
  const processorStateControlEnabledRef = useRef(true)
  const [nvidiaTelemetryEnabled, setNvidiaTelemetryEnabledState] = useState(true)
  const nvidiaTelemetryEnabledRef = useRef(true)
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
  const [backendVersion, setBackendVersion] = useState('0.13.0')
  const [updateStatus, setUpdateStatus] = useState<UpdateStatus | null>(null)
  const [updateActionPending, setUpdateActionPending] = useState<UpdateAction | null>(null)
  const [updateActionMessage, setUpdateActionMessage] = useState<string | null>(null)
  const autoUpdateCheckTriggeredRef = useRef(false)
  const updateNotificationKeyRef = useRef<string | null>(null)
  const [statusMessage, setStatusMessage] = useState(
    'Desktop backend starting. Loading persisted AeroForge state.',
  )
  const [settingsActionPending, setSettingsActionPending] = useState<
    null | 'smart-charge' | 'blue-light' | 'boot-logo' | 'refresh-rate' | 'nvidia-telemetry'
  >(null)
  const [glowTarget, setGlowTarget] = useState<string>('turbo')
  const [shellStatus, setShellStatus] = useState('Browser preview shell')
  const [serviceConnected, setServiceConnected] = useState(false)
  const serviceConnectedRef = useRef(false)
  const [serviceStatus, setServiceStatus] = useState<ServiceStatus | null>(null)
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
  const finalizeCustomCurveEditRef = useRef<
    (nextCurves: CurveSet, options?: FinalizeCustomCurveOptions) => Promise<void>
  >(async () => {})
  const pushTransportDebugEventRef = useRef<(message: string) => void>(() => {})
  const pushPollHeartbeatEventRef = useRef<(message: string) => void>(() => {})
  const runUpdateCheckRef = useRef<
    ((manual: boolean, channelOverride?: UpdateChannel) => Promise<UpdateStatus>) | null
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
    liveControlSnapshot?.currentCpuFanSpeedPercent ?? null
  const displayedGpuFanTargetPercent =
    liveControlSnapshot?.currentGpuFanSpeedPercent ?? null
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
  const cpuPowerMeterDetail = liveCpuPowerLimitLabel ?? 'No PL readback'
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

  function commitCustomCurves(next: CurveSet | ((current: CurveSet) => CurveSet)) {
    setCustomCurves((current) => {
      const resolved =
        typeof next === 'function' ? (next as (current: CurveSet) => CurveSet)(current) : next
      const normalized = duplicateCurveSet(resolved)
      customCurvesRef.current = normalized
      return normalized
    })
  }

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
    },
    [],
  )

  async function persistStagedControls(overrides?: PersistControlOverrides) {
    const nextActivePowerProfile = overrides?.activePowerProfile ?? activePowerProfile
    const nextActiveFanProfile = overrides?.activeFanProfile ?? activeFanProfile
    const nextCustomProcessorState =
      overrides?.customProcessorState ?? customProcessorStateRef.current
    const nextCustomPowerBase = overrides?.customPowerBase ?? customPowerBaseRef.current
    const nextCustomCurves = overrides?.customCurves ?? customCurvesRef.current
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

  useEffect(() => {
    if (!initializedPersistenceRef.current) {
      initializedPersistenceRef.current = true
      return
    }

    void persistStagedControlsRef.current()
  }, [
    activePowerProfile,
    customProcessorState.min,
    customProcessorState.max,
    customPowerBase,
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

      const nextScale = Math.min(1, availableHeight / naturalHeight, availableWidth / naturalWidth)
      const clampedScale = Number.isFinite(nextScale) ? Math.max(0.78, nextScale) : 1
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
        pushDebugEvent(
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

  function queuePerformanceEvent(
    eventType: string,
    detail: string,
    payload: Record<string, unknown> = {},
  ) {
    if (!isDesktopRuntime()) {
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

    console.debug(`[AeroForge debug] ${entry}`)

    const next = [entry, ...debugEventsRef.current].slice(0, 8)
    debugEventsRef.current = next

    if (activeTabRef.current === 'debug') {
      setDebugEvents(next)
    }
  }

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
        pushDebugEvent(`auto refresh-rate sync failed: ${describeError(error)}`)
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
          pushDebugEvent(
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
  }, [])

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
        const telemetry = bootstrap.telemetry
        const liveControls = bootstrap.liveControls
        liveControlsMs = 0

        if (!cancelled) {
          setBackendCapabilities(bootstrap.capabilities)
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
          )
          setBackendVersion(runtime.backendVersion)
          setUpdateStatus(updater)
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
          service.connected ? 'bootstrap connected' : 'bootstrap fallback',
          {
            totalMs: performance.now() - bootstrapStartedAt,
            backendBootstrapMs,
            persistenceMs,
            updaterMs,
            liveControlsMs,
            serviceConnected: service.connected,
            workerCount: service.workerCount,
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
      return document.visibilityState === 'hidden'
        ? HIDDEN_BACKEND_POLL_INTERVAL_MS
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

    if (!checkForUpdatesOnLaunch) {
      return
    }

    autoUpdateCheckTriggeredRef.current = true
    void runUpdateCheckRef.current?.(false)
  }, [checkForUpdatesOnLaunch])

  function pulseControl(target: string) {
    setGlowTarget(target)
    window.clearTimeout((pulseControl as typeof pulseControl & { timer?: number }).timer)
    ;(pulseControl as typeof pulseControl & { timer?: number }).timer = window.setTimeout(
      () => setGlowTarget(''),
      1200,
    )
  }

  async function handlePowerProfile(profileId: PowerProfile['id']) {
    if (profileId === 'custom' && customPowerApplyTimerRef.current !== null) {
      window.clearTimeout(customPowerApplyTimerRef.current)
      customPowerApplyTimerRef.current = null
      customPowerApplyRevisionRef.current += 1
    }

    const nextProcessorState = getProcessorStateForPowerProfile(
      profileId,
      customProcessorStateRef.current,
    )
    const profileName = powerProfiles.find((profile) => profile.id === profileId)?.name ?? 'Power'

    if (profileId === 'custom') {
      customProcessorStateRef.current = nextProcessorState
      setCustomProcessorState(nextProcessorState)
    }

    const hasService = serviceConnectedRef.current
    const processorPolicyEnabled = processorStateControlEnabledRef.current

    setActivePowerProfile(profileId)
    setStatusMessage(
      hasService
        ? processorPolicyEnabled
          ? `${profileName} profile apply requested.`
          : `${profileName} profile apply requested. CPU min/max writes are off.`
        : `${profileName} profile staged in the frontend preview.`,
    )
    pulseControl(profileId)
    queuePerformanceEvent('power-profile-select', profileId, {
      serviceConnected: hasService,
    })

    if (!hasService) {
      await waitForNextPaint()
      await persistStagedControls({
        activePowerProfile: profileId,
        ...(profileId === 'custom' ? { customProcessorState: nextProcessorState } : {}),
      })
      return
    }

    if (powerProfileApplyInFlightRef.current) {
      queuedPowerProfileRef.current = profileId
      setStatusMessage(`${profileName} profile queued after the current hardware apply finishes.`)
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
      const queuedProfile = queuedPowerProfileRef.current
      const resultSuperseded = queuedProfile !== null && queuedProfile !== profileId

      if (!resultSuperseded) {
        applyBackendControlSnapshot(
          mergeControlsWithLiveSnapshot(updatedControls, liveControls),
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
        )
      }
      queuePerformanceEvent('power-profile-apply-finished', profileId, {
        totalMs: performance.now() - applyStartedMs,
        refreshMs: refreshed.refreshMs,
        superseded: resultSuperseded,
        queuedProfile,
      })

      if (!resultSuperseded) {
        if (!processorPolicyEnabled) {
          setStatusMessage(
            `${profileName} profile applied through the AeroForge service. CPU min/max writes were skipped by Settings.`,
          )
        } else {
          setStatusMessage(
        `${profileName} profile applied through the AeroForge service${
          liveControls?.processorStateReadback
            ? ` and verified as AC ${liveControls.processorStateReadback.ac.minPercent}/${liveControls.processorStateReadback.ac.maxPercent} • DC ${liveControls.processorStateReadback.dc.minPercent}/${liveControls.processorStateReadback.dc.maxPercent}.`
            : '.'
        }`,
          )
        }
      }
    } catch (error) {
      queuePerformanceEvent('power-profile-apply-failed', profileId, {
        totalMs: performance.now() - applyStartedMs,
        error: describeError(error),
      })
      setStatusMessage(
        `Power profile apply failed: ${error instanceof Error ? error.message : String(error)}`,
      )
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
    if (!serviceConnectedRef.current) {
      setStatusMessage(`${reason} in the preview.`)
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
    if (!serviceConnectedRef.current) {
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

      applyBackendControlSnapshot(
        mergeControlsWithLiveSnapshot(updatedControls, liveControls),
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
      )
      liveControlSnapshotRef.current = liveControls
      setLiveControlSnapshot(liveControls)

      setStatusMessage(
        !processorPolicyEnabled
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
    let hasService = serviceConnectedRef.current

    setActiveFanProfile(profileId)
    setStatusMessage(
      hasService
        ? `${profileName} fan mode apply requested.`
        : `${profileName} fan mode staged in the frontend preview.`,
    )
    pulseControl(profileId)
    queuePerformanceEvent('fan-profile-select', profileId, {
      serviceConnected: hasService,
    })

    if (!hasService) {
      const reconnectStartedMs = performance.now()

      try {
        const snapshot = await getBackendPollSnapshot()
        hasService = snapshot.service.connected
        serviceConnectedRef.current = snapshot.service.connected
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
        queuePerformanceEvent('fan-profile-service-recheck', profileId, {
          reconnectMs: performance.now() - reconnectStartedMs,
          serviceConnected: snapshot.service.connected,
          workerCount: snapshot.service.workerCount,
        })
        if (hasService) {
          setStatusMessage(`${profileName} fan mode apply requested after service reconnect.`)
        }
      } catch (error) {
        queuePerformanceEvent('fan-profile-service-recheck-failed', profileId, {
          reconnectMs: performance.now() - reconnectStartedMs,
          error: describeError(error),
        })
      }
    }

    if (!hasService) {
      await waitForNextPaint()
      await persistStagedControls({ activeFanProfile: profileId })
      return
    }

    if (fanProfileApplyInFlightRef.current) {
      queuedFanProfileRef.current = profileId
      setStatusMessage(`${profileName} fan mode queued after the current hardware apply finishes.`)
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
      const queuedProfile = queuedFanProfileRef.current
      const resultSuperseded = queuedProfile !== null && queuedProfile !== profileId

      if (!resultSuperseded) {
        applyBackendControlSnapshot(
        mergeControlsWithLiveSnapshot(result.controls, liveControls),
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
      )
      }
      queuePerformanceEvent('fan-profile-apply-finished', profileId, {
        totalMs: performance.now() - applyStartedMs,
        refreshMs: refreshed.refreshMs,
        superseded: resultSuperseded,
        queuedProfile,
      })
      if (!resultSuperseded) {
        setStatusMessage(result.detail)
      }
    } catch (error) {
      queuePerformanceEvent('fan-profile-apply-failed', profileId, {
        totalMs: performance.now() - applyStartedMs,
        error: describeError(error),
      })
      setStatusMessage(`Fan profile apply failed: ${describeError(error)}`)
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

  async function applyCustomCurvesToService(nextCurves: CurveSet) {
    const result = await applyCustomFanCurves(toBackendCurveSet(nextCurves))
    const liveControls = await getLiveControlSnapshot().catch(() => null)

    applyBackendControlSnapshot(
      result.controls,
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
    )
    liveControlSnapshotRef.current = liveControls
    setLiveControlSnapshot(liveControls)
    return result.detail
  }

  async function finalizeCustomCurveEdit(
    nextCurves: CurveSet,
    options?: FinalizeCustomCurveOptions,
  ) {
    const nextActiveFanProfile = options?.activateCustom ? 'custom' : activeFanProfile
    const nextFanSyncLockEnabled = options?.fanSyncLockState ?? fanSyncLockEnabled

    if (options?.activateCustom) {
      setActiveFanProfile('custom')
      pulseControl('custom')
    }

    await persistStagedControls({
      activeFanProfile: nextActiveFanProfile,
      customCurves: nextCurves,
      fanSyncLockEnabled: nextFanSyncLockEnabled,
    })

    if (!serviceConnected) {
      setStatusMessage(
        options?.statusMessage ??
          'Custom fan curves saved to the preview. Connect the AeroForge service to apply them on hardware.',
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
      void persistStagedControls({ fanSyncLockEnabled: false })
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
      applyBackendControlSnapshot(
        result.controls,
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
      )
      await persistStagedControls({
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
      applyBackendControlSnapshot(
        result.controls,
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
      )
      await persistStagedControls({
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
      const message = describeError(error)
      const detail = `Update check failed: ${message}`
      setUpdateActionMessage(detail)
      setStatusMessage(detail)
      throw error
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
      await persistStagedControls({ blueLightFilterEnabled: nextEnabled })
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
      await persistStagedControls({ smartChargingEnabled: nextEnabled })
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
    await persistStagedControls({ processorStateControlEnabled: nextEnabled })
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
      applyBackendControlSnapshot(
        result.controls,
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
      )
      setStatusMessage(result.detail)
    } catch (error) {
      commitNvidiaTelemetryEnabled(previousEnabled)
      setStatusMessage(`NVIDIA telemetry setting failed: ${describeError(error)}`)
    } finally {
      setSettingsActionPending((current) => (current === 'nvidia-telemetry' ? null : current))
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
    void persistStagedControls({
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
    void persistStagedControls({
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

      applyBackendControlSnapshot(
        result.controls,
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
      )

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
          </nav>
        </header>

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
                        Custom mode stores edits immediately and refreshes the curve-derived fan target every 1 second.
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
                                      updateStatus.canInstallUpdate
                                        ? ''
                                        : ' - not installable for the current release.'
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
                  ) : (
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
                  )}
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
              </div>

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
                    {debugEvents.map((event) => (
                      <li key={event}>{event}</li>
                    ))}
                  </ul>
                </div>
              </section>
            </section>
          )}
        </div>
      </main>
    </div>
  )
}

type MetricCardProps = {
  label: string
  value: string
  detail: string
}

function MetricCard({ label, value, detail }: MetricCardProps) {
  return (
    <div className="metric-card">
      <span>{label}</span>
      <strong>{value}</strong>
      <small>{detail}</small>
    </div>
  )
}

type HomeFanReadoutCardProps = {
  label: string
  value: number | null
  detail: string
}

function HomeFanReadoutCard({ label, value, detail }: HomeFanReadoutCardProps) {
  return (
    <div className="home-fan-card">
      <div className="home-fan-card__visual" aria-hidden="true">
        <div className="home-fan-card__hub" />
      </div>
      <div className="home-fan-card__copy">
        <span className="eyebrow">{label}</span>
        <strong>{value == null ? 'Offline' : formatTelemetryValue(value)}</strong>
        <small>{value == null ? '' : 'RPM'}</small>
        <p>{detail}</p>
      </div>
    </div>
  )
}

type HomeTemperatureDialProps = {
  label: string
  identity?: string
  value: number | null
  details?: Array<{
    label: string
    value: number | null
    suffix?: string
    displayValue?: string
  }>
}

function HomeTemperatureDial({
  label,
  identity,
  value,
  details = [],
}: HomeTemperatureDialProps) {
  const dialSize = 132
  const dialStroke = 16
  const dialRadius = (dialSize - dialStroke) / 2
  const safeValue = value == null ? 0 : clamp(value, 0, 100)
  const greenLength = clamp(safeValue, 0, 50)
  const orangeLength = clamp(safeValue - 50, 0, 20)
  const redLength = clamp(safeValue - 70, 0, 30)

  return (
    <div className={`home-temp-card ${details.length > 0 ? 'home-temp-card--detailed' : ''}`}>
      <div className="home-temp-card__heading">
        <span className="eyebrow">{label}</span>
        {identity && <strong>{identity}</strong>}
      </div>
      <div className="home-temp-card__visual">
        <svg
          className="home-temp-card__ring"
          viewBox={`0 0 ${dialSize} ${dialSize}`}
          aria-hidden="true"
        >
          <circle
            className="home-temp-card__track"
            cx={dialSize / 2}
            cy={dialSize / 2}
            r={dialRadius}
            pathLength="100"
          />
          <circle
            className="home-temp-card__segment home-temp-card__segment--green"
            cx={dialSize / 2}
            cy={dialSize / 2}
            r={dialRadius}
            pathLength="100"
            style={{
              strokeDasharray: `${greenLength} ${100 - greenLength}`,
              strokeDashoffset: '0',
            }}
          />
          <circle
            className="home-temp-card__segment home-temp-card__segment--orange"
            cx={dialSize / 2}
            cy={dialSize / 2}
            r={dialRadius}
            pathLength="100"
            style={{
              strokeDasharray: `${orangeLength} ${100 - orangeLength}`,
              strokeDashoffset: '-50',
            }}
          />
          <circle
            className="home-temp-card__segment home-temp-card__segment--red"
            cx={dialSize / 2}
            cy={dialSize / 2}
            r={dialRadius}
            pathLength="100"
            style={{
              strokeDasharray: `${redLength} ${100 - redLength}`,
              strokeDashoffset: '-70',
            }}
          />
        </svg>
        <div className="home-temp-card__content">
          <strong>
            {formatTelemetryValue(value)}
            {value == null ? '' : <span className="home-temp-card__unit"> C</span>}
          </strong>
        </div>
      </div>
      {details.length > 0 && (
        <div className="home-temp-card__details">
          {details.map((detail) => (
            <div className="home-temp-card__detail-row" key={detail.label}>
              <span>{detail.label}</span>
              <strong>
                {detail.displayValue ?? formatTelemetryValue(detail.value)}
                {detail.displayValue != null || detail.value == null ? '' : detail.suffix ?? ''}
              </strong>
            </div>
          ))}
        </div>
      )}
    </div>
  )
}

type BootSplashPreviewProps = {
  art: BootArt
  compact?: boolean
}

function BootSplashPreview({ art, compact = false }: BootSplashPreviewProps) {
  return (
    <div
      className={`branding-preview__placeholder boot-splash ${art.palette} boot-splash--${art.layout} ${
        compact ? 'is-compact' : ''
      }`}
    >
      <div className="boot-splash__glow" />
      <img className="boot-splash__mark" src={aeroforgeMark} alt="" aria-hidden="true" />
      <div className="boot-splash__copy">
        <strong>{art.headline}</strong>
        <span>{art.subline}</span>
      </div>
    </div>
  )
}

type ProcessorStateControlProps = {
  label: string
  min?: number
  value: number
  onChange: (value: number) => void
}

function ProcessorStateControl({
  label,
  min = 5,
  value,
  onChange,
}: ProcessorStateControlProps) {
  return (
    <label className="power-setting">
      <div className="power-setting__header">
        <span>{label}</span>
        <strong>{value}%</strong>
      </div>

      <input
        className="power-setting__slider"
        type="range"
        min={min}
        max="100"
        step="1"
        value={value}
        onChange={(event) => onChange(Number(event.target.value))}
      />

      <input
        className="power-setting__input"
        type="number"
        min={min}
        max="100"
        step="1"
        value={value}
        onChange={(event) => onChange(Number(event.target.value))}
      />
    </label>
  )
}

type OverclockSliderProps = {
  label: string
  unit: string
  value: number
  min: number
  max: number
  step: number
  disabled?: boolean
  statusLabel?: string
  note?: string
  onChange: (value: number) => void
}

function OverclockSlider({
  label,
  unit,
  value,
  min,
  max,
  step,
  disabled = false,
  statusLabel,
  note,
  onChange,
}: OverclockSliderProps) {
  return (
    <label className={`power-setting ${disabled ? 'is-disabled' : ''}`}>
      <div className="power-setting__header">
        <div className="power-setting__title">
          <span>{label}</span>
          {statusLabel && <em className="power-setting__badge">{statusLabel}</em>}
        </div>
        <strong>
          {value > 0 && unit !== 'C' ? '+' : ''}
          {value}
          {unit}
        </strong>
      </div>

      <input
        className="power-setting__slider"
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        disabled={disabled}
        onChange={(event) => onChange(Number(event.target.value))}
      />

      <input
        className="power-setting__input"
        type="number"
        min={min}
        max={max}
        step={step}
        value={value}
        disabled={disabled}
        onChange={(event) => onChange(Number(event.target.value))}
      />

      {note && <small className="power-setting__note">{note}</small>}
    </label>
  )
}

type FanCurvePanelProps = {
  title: string
  target: CurveTarget
  points: CurvePoint[]
  editable: boolean
  chartRef: (node: SVGSVGElement | null) => void
  onPointDown: (index: number) => void
  syncLockEnabled: boolean
  onSyncLockToggle: () => void
  onSecondaryAction: () => void
  secondaryLabel: string
}

function FanCurvePanel({
  title,
  target,
  points,
  editable,
  chartRef,
  onPointDown,
  syncLockEnabled,
  onSyncLockToggle,
  onSecondaryAction,
  secondaryLabel,
}: FanCurvePanelProps) {
  return (
    <section className="fan-curve-card">
      <div className="fan-curve-card__header">
        <h3>{title}</h3>
        <span>{target.toUpperCase()} thermal zone</span>
      </div>

      <div className="fan-curve-card__chart-wrap">
        <svg
          ref={chartRef}
          className={`curve-chart ${editable ? 'is-editable' : ''}`}
          viewBox={`0 0 ${chartWidth} ${chartHeight}`}
          aria-label={`${target.toUpperCase()} fan curve editor`}
        >
          {[speedMin, 25, 50, 75, 100].map((speed) => {
            const y =
              chartHeight -
              chartPadding -
              ((speed - speedMin) / (speedMax - speedMin)) * (chartHeight - chartPadding * 2)

            return (
              <g key={`${target}-speed-${speed}`}>
                <line
                  className="curve-chart__grid"
                  x1={chartPadding}
                  x2={chartWidth - chartPadding}
                  y1={y}
                  y2={y}
                />
              </g>
            )
          })}

          {[30, 45, 60, 75, 90].map((temp) => {
            const x =
              chartPadding +
              ((temp - tempMin) / (tempMax - tempMin)) * (chartWidth - chartPadding * 2)

            return (
              <g key={`${target}-temp-${temp}`}>
                <line
                  className="curve-chart__grid curve-chart__grid--vertical"
                  x1={x}
                  x2={x}
                  y1={chartPadding}
                  y2={chartHeight - chartPadding}
                />
              </g>
            )
          })}

          <path className="curve-chart__glow" d={buildCurvePath(points)} />
          <path className="curve-chart__path" d={buildCurvePath(points)} />

          {points.map((point, index) => {
            const { x, y } = pointToChart(point)
            return (
              <g key={`${target}-${index}`}>
                <circle className="curve-chart__halo" cx={x} cy={y} r="14" />
                <circle
                  className="curve-chart__point"
                  cx={x}
                  cy={y}
                  r="8"
                  onPointerDown={(event) => {
                    if (!editable) {
                      return
                    }
                    event.preventDefault()
                    event.currentTarget.setPointerCapture(event.pointerId)
                    onPointDown(index)
                  }}
                />
              </g>
            )
          })}
        </svg>

        <div className="fan-curve-card__axis-note fan-curve-card__axis-note--left">
          fan %
        </div>
        <div className="fan-curve-card__axis-note fan-curve-card__axis-note--right">
          temp C
        </div>
        <div className="fan-curve-card__axis-note fan-curve-card__axis-note--bottom-left">
          30C
        </div>
        <div className="fan-curve-card__axis-note fan-curve-card__axis-note--bottom-right">
          90C
        </div>
      </div>

      <div className="fan-curve-card__points">
        {points.map((point, index) => (
          <div className="fan-curve-chip" key={`${target}-chip-${index}`}>
            <span>P{index + 1}</span>
            <strong>{point.temp}C</strong>
            <strong>{point.speed}%</strong>
          </div>
        ))}
      </div>

      <div className="fan-curve-actions">
        <button
          className={`button button--toggle ${syncLockEnabled ? 'is-active' : ''}`}
          onClick={onSyncLockToggle}
          aria-pressed={syncLockEnabled}
        >
          Sync Lock
        </button>
        <button className="button" onClick={onSecondaryAction}>
          {secondaryLabel}
        </button>
      </div>
    </section>
  )
}

export default App

