// Pure conversions between the backend ControlSnapshot and the UI's GPU tuning,
// OC slot and fan curve shapes.
import type {
  ControlSnapshot,
  CustomPowerBaseId,
  FanProfileId,
  LiveControlSnapshot,
  PowerProfileId,
} from './backend'
import { duplicateCurveSet, type CurveSet } from './curves'

export type GpuTuningState = {
  coreClock: number
  memoryClock: number
  voltageOffset: number
  powerLimit: number
  tempLimit: number
}

export type OcProfileSlot = {
  id: string
  label: string
  name: string
  strap: string
  settings: GpuTuningState
  isCustom?: boolean
}

export const defaultGpuOverclock: GpuTuningState = {
  coreClock: 0,
  memoryClock: 0,
  voltageOffset: 0,
  powerLimit: 100,
  tempLimit: 83,
}

export const nitroSenseExtremeGpuOverclock: GpuTuningState = {
  coreClock: 100,
  memoryClock: 200,
  voltageOffset: 0,
  powerLimit: 100,
  tempLimit: 83,
}

export const builtInOcProfileSlots: OcProfileSlot[] = [
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

export const defaultCustomOcSlot: OcProfileSlot = {
  id: 'custom-user',
  label: 'P5',
  name: 'Custom Preset',
  strap: 'User-saved GPU tuning',
  settings: {
    ...defaultGpuOverclock,
  },
  isCustom: true,
}

export function buildCustomOcStrap(settings: GpuTuningState) {
  const core = settings.coreClock >= 0 ? `+${settings.coreClock}` : `${settings.coreClock}`
  const memory =
    settings.memoryClock >= 0 ? `+${settings.memoryClock}` : `${settings.memoryClock}`
  const voltage =
    settings.voltageOffset >= 0
      ? `+${settings.voltageOffset}mV`
      : `${settings.voltageOffset}mV`

  return `Core ${core} / Mem ${memory} / ${voltage}`
}

export function fromBackendGpuTuningState(
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

export function toBackendGpuTuningState(tuning: GpuTuningState): ControlSnapshot['gpuTuning'] {
  return {
    coreClockMhz: tuning.coreClock,
    memoryClockMhz: tuning.memoryClock,
    voltageOffsetMv: tuning.voltageOffset,
    powerLimitPercent: tuning.powerLimit,
    tempLimitC: tuning.tempLimit,
  }
}

export function fromBackendOcPreset(preset: ControlSnapshot['ocPresets'][number]): OcProfileSlot {
  return {
    id: preset.id,
    label: preset.label,
    name: preset.name,
    strap: preset.strap,
    settings: fromBackendGpuTuningState(preset.settings),
    isCustom: preset.isCustom,
  }
}

export function toBackendOcPreset(slot: OcProfileSlot): ControlSnapshot['ocPresets'][number] {
  return {
    id: slot.id,
    label: slot.label,
    name: slot.name,
    strap: slot.strap,
    settings: toBackendGpuTuningState(slot.settings),
    isCustom: Boolean(slot.isCustom),
  }
}

export function toBackendCurveSet(curves: CurveSet): ControlSnapshot['fanCurves'] {
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

export function fromBackendCurveSet(curves: ControlSnapshot['fanCurves']): CurveSet {
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

export function mergeControlsWithLiveSnapshot(
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

export function buildControlSnapshotForPersistence(input: {
  activePowerProfile: PowerProfileId
  activeFanProfile: FanProfileId
  customProcessorState: { min: number; max: number }
  customPowerBase: CustomPowerBaseId
  gpuOverclock: GpuTuningState
  ocProfileSlots: OcProfileSlot[]
  activeOcSlot: string
  ocApplyState: 'staged' | 'live'
  ocTuningLocked: boolean
  customCurves: CurveSet
  fanPresets: ControlSnapshot['fanPresets']
  powerPresets: ControlSnapshot['powerPresets']
  quickPresets: ControlSnapshot['quickPresets']
  fanSyncLockEnabled: boolean
  smartChargingEnabled: boolean
  processorStateControlEnabled: boolean
  nvidiaTelemetryEnabled: boolean
  keepUiPrewarmed: boolean
  autoRefreshRateOnBatteryEnabled: boolean
  autoRefreshRateRestoreHz: number | null
  autoPerformanceEnabled: boolean
  usbPowerEnabled: boolean
  blueLightFilterEnabled: boolean
  selectedBootArt: string
  customBootFilename: string
  updateChannel: ControlSnapshot['personalSettings']['updateChannel']
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
      keepUiPrewarmed: input.keepUiPrewarmed,
      blueLightFilterEnabled: input.blueLightFilterEnabled,
      autoRefreshRateOnBatteryEnabled: input.autoRefreshRateOnBatteryEnabled,
      autoRefreshRateRestoreHz: input.autoRefreshRateRestoreHz,
      autoPerformanceEnabled: input.autoPerformanceEnabled,
      selectedBootArt: input.selectedBootArt as ControlSnapshot['personalSettings']['selectedBootArt'],
      customBootFilename: input.customBootFilename,
      updateChannel: input.updateChannel,
      checkForUpdatesOnLaunch: input.checkForUpdatesOnLaunch,
    },
  }
}
