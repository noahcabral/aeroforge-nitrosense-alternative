import { invoke } from '@tauri-apps/api/core'

export type ShellStatus = {
  shell: string
  backendVersion: string
}

export type ServiceWorkerStatus = {
  name: string
  state: string
  intervalSeconds: number
  lastUpdateUnix: number
  lastError: string | null
}

export type ServiceStatus = {
  connected: boolean
  pipeName: string
  serviceName: string
  version: string | null
  stateDir: string | null
  supervisorFile: string | null
  workerCount: number
  updatedAtUnix: number | null
  workers: ServiceWorkerStatus[]
  detail: string
}

export type FeatureSupport = {
  available: boolean
  writable: boolean
  requiresElevation: boolean
}

export type CapabilitySnapshot = {
  powerProfiles: FeatureSupport
  fanProfiles: FeatureSupport
  fanCurves: FeatureSupport
  smartCharging: FeatureSupport
  usbPower: FeatureSupport
  blueLightFilter: FeatureSupport
  gpuTuning: FeatureSupport
  bootLogo: FeatureSupport
  notes: string[]
  cacheUpdatedAtUnix?: number | null
}

export type ControlLayerState = 'applied' | 'unavailable' | 'failed' | 'skipped' | 'unknown'

export type ControlLayerStatus = {
  id: string
  label: string
  state: ControlLayerState
  detail: string
}

export type WindowsServiceReadiness = {
  name: string
  installed: boolean
  running: boolean
  startType: string | null
  detail: string
}

export type WmiReadiness = {
  service: WindowsServiceReadiness
  disabled: boolean
  needsRepair: boolean
  detail: string
}

export type AcerControlReadiness = {
  systemVendor: string | null
  systemModel: string | null
  biosVersion: string | null
  acerHardware: boolean
  anv15_51: boolean
  acerGamingWmiPresent: boolean
  detail: string
}

export type HardwareReadinessSnapshot = {
  service: WindowsServiceReadiness
  wmi: WmiReadiness
  acer: AcerControlReadiness
  ready: boolean
  repairAvailable: boolean
  detail: string
}

export type RepairLaunchResult = {
  launched: boolean
  elevated: boolean
  detail: string
  logPath: string | null
}

export type ServiceHealResult = {
  requested: boolean
  taskAvailable: boolean
  serviceRunning: boolean
  detail: string
}

export type PowerProfileId = 'battery-guard' | 'balanced' | 'performance' | 'turbo' | 'custom'
export type CustomPowerBaseId = 'balanced' | 'performance' | 'turbo'
export type FanProfileId = 'auto' | 'cool' | 'max' | 'custom'
export type BootArtId = 'ember' | 'arc' | 'slate' | 'custom'
export type ApplyState = 'staged' | 'live'

export type ProcessorStateSettings = {
  minPercent: number
  maxPercent: number
}

export type ProcessorStateReadback = {
  ac: ProcessorStateSettings
  dc: ProcessorStateSettings
}

export type GpuTuningState = {
  coreClockMhz: number
  memoryClockMhz: number
  voltageOffsetMv: number
  powerLimitPercent: number
  tempLimitC: number
}

export type FanCurvePoint = {
  tempC: number
  speedPercent: number
}

export type FanCurveSet = {
  cpu: FanCurvePoint[]
  gpu: FanCurvePoint[]
}

export type OcPreset = {
  id: string
  label: string
  name: string
  strap: string
  settings: GpuTuningState
  isCustom: boolean
}

export type FanControlPreset = {
  id: string
  name: string
  profileId: FanProfileId
  curves: FanCurveSet
}

export type PowerControlPreset = {
  id: string
  name: string
  profileId: PowerProfileId
  customBase: CustomPowerBaseId
  processorState: ProcessorStateSettings
}

export type QuickControlPreset = {
  id: string
  name: string
  fanPresetId: string | null
  powerPresetId: string | null
}

export type PersonalSettings = {
  smartChargingEnabled: boolean
  usbPowerEnabled: boolean
  processorStateControlEnabled: boolean
  nvidiaTelemetryEnabled: boolean
  keepUiPrewarmed: boolean
  blueLightFilterEnabled: boolean
  autoRefreshRateOnBatteryEnabled: boolean
  autoRefreshRateRestoreHz: number | null
  /** Opt-in Automatic Performance: load/thermal/AC-driven power + safe GPU offsets. */
  autoPerformanceEnabled: boolean
  selectedBootArt: BootArtId
  customBootFilename: string
  updateChannel: 'stable' | 'preview'
  checkForUpdatesOnLaunch: boolean
}

export type ControlSnapshot = {
  activePowerProfile: PowerProfileId
  activeFanProfile: FanProfileId
  customProcessorState: ProcessorStateSettings
  customPowerBase: CustomPowerBaseId
  gpuTuning: GpuTuningState
  ocPresets: OcPreset[]
  activeOcSlot: string
  ocApplyState: ApplyState
  ocTuningLocked: boolean
  fanCurves: FanCurveSet
  fanPresets: FanControlPreset[]
  powerPresets: PowerControlPreset[]
  quickPresets: QuickControlPreset[]
  fanSyncLockEnabled: boolean
  personalSettings: PersonalSettings
}

export type QuietAutoFanMap = {
  lastPercent: number | null
  lastRpm: number | null
  idlePercent: number | null
  elevatedPercent: number | null
}

export type QuietAutoFanCalibration = {
  cpu: QuietAutoFanMap
  gpu: QuietAutoFanMap
  lastTargetRpm: number | null
  updatedAtUnix: number | null
}

export type QuietAutoThermalWarning = {
  active: boolean
  sensor: string | null
  tempC: number | null
  message: string | null
  updatedAtUnix: number | null
}

export type FanSpeedCalibrationPoint = {
  percent: number
  cpuRpm: number | null
  gpuRpm: number | null
  cpuTempC: number | null
  gpuTempC: number | null
  systemTempC: number | null
  sampledAtUnix: number
}

export type FanSpeedCalibrationSnapshot = {
  running: boolean
  status: string
  startedAtUnix: number | null
  updatedAtUnix: number | null
  completedAtUnix: number | null
  currentPercent: number | null
  settleSeconds: number
  points: FanSpeedCalibrationPoint[]
  lastError: string | null
}

export type LiveControlSnapshot = {
  service: string
  cacheUpdatedAtUnix?: number | null
  powerApplySupported: boolean
  gpuTuningApplySupported: boolean
  fanApplySupported: boolean
  fanCurveApplySupported: boolean
  activePowerProfile: PowerProfileId | null
  processorState: ProcessorStateSettings | null
  processorStateReadback: ProcessorStateReadback | null
  processorStateDriftDetected: boolean
  lastAppliedAtUnix: number | null
  lastApplyDetail: string
  lastError: string | null
  lastPowerLayerStatuses: ControlLayerStatus[]
  activeFanProfile: FanProfileId | null
  activeFanCurves: FanCurveSet | null
  currentCpuFanSpeedPercent: number | null
  currentGpuFanSpeedPercent: number | null
  lastFanAppliedAtUnix: number | null
  lastFanApplyDetail: string
  lastFanError: string | null
  lastFanLayerStatuses: ControlLayerStatus[]
  lastFanReadback: unknown | null
  quietAutoFanCalibration: QuietAutoFanCalibration
  quietAutoThermalWarning: QuietAutoThermalWarning | null
  fanSpeedCalibration: FanSpeedCalibrationSnapshot
  bootLogoApplySupported: boolean
  lastBootLogoAppliedAtUnix: number | null
  lastBootLogoApplyDetail: string
  lastBootLogoError: string | null
  lastBootLogoReadback: unknown | null
}

export type TelemetrySnapshot = {
  cacheUpdatedAtUnix?: number | null
  cpuTempC: number
  cpuTempAverageC: number | null
  cpuTempLowestCoreC: number | null
  cpuTempHighestCoreC: number | null
  gpuTempC: number
  systemTempC: number
  cpuUsagePercent: number
  gpuUsagePercent: number
  gpuMemoryUsagePercent: number | null
  gpuPowerDrawW: number | null
  gpuPowerLimitW: number | null
  gpuPowerDefaultLimitW: number | null
  gpuPowerMinLimitW: number | null
  gpuPowerMaxLimitW: number | null
  cpuPackagePowerW: number | null
  cpuPl1W: number | null
  cpuPl1Enabled: boolean | null
  cpuPl2W: number | null
  cpuPl2Enabled: boolean | null
  cpuPowerLimitLocked: boolean | null
  cpuName: string | null
  cpuBrand: string | null
  gpuName: string | null
  gpuBrand: string | null
  systemVendor: string | null
  systemModel: string | null
  cpuClockMhz: number
  gpuClockMhz: number
  cpuFanRpm: number
  gpuFanRpm: number
  batteryPercent: number
  batteryLifeRemainingSec: number | null
  acPluggedIn: boolean
}

export type BackendPollTimings = {
  totalMs: number
  serviceMs: number
  telemetryMs: number
  liveControlsMs: number
}

export type BackendPollSnapshot = {
  service: ServiceStatus
  telemetry: TelemetrySnapshot
  liveControls: LiveControlSnapshot | null
  timings: BackendPollTimings
}

export type GpuTuningApplyResult = {
  controls: ControlSnapshot
  appliedAtUnix: number
  detail: string
}

export type FanControlApplyResult = {
  controls: ControlSnapshot
  appliedAtUnix: number
  detail: string
}

export type BootLogoApplyResult = {
  controls: ControlSnapshot
  appliedAtUnix: number
  detail: string
}

export type BlueLightApplyResult = {
  controls: ControlSnapshot
  appliedAtUnix: number
  gainId: number
  detail: string
}

export type SmartChargeApplyResult = {
  controls: ControlSnapshot
  appliedAtUnix: number
  batteryHealthy: number
  detail: string
}

export type DisplayRefreshApplyResult = {
  controls: ControlSnapshot
  appliedAtUnix: number
  enabled: boolean
  onBattery: boolean
  currentHz: number
  appliedHz: number | null
  restoreHz: number | null
  detail: string
}

export type NvidiaTelemetryApplyResult = {
  controls: ControlSnapshot
  enabled: boolean
  detail: string
}

export type BackendBootstrap = {
  shell: ShellStatus
  service: ServiceStatus
  capabilities: CapabilitySnapshot
  hardwareReadiness: HardwareReadinessSnapshot
  controls: ControlSnapshot
  telemetry: TelemetrySnapshot
  liveControls: LiveControlSnapshot | null
  persistence: PersistenceStatus
  updateStatus: UpdateStatus
}

export type PersistenceStatus = {
  configFile: string
  initializedFromDisk: boolean
}

export type UpdateStatus = {
  repoSlug: string
  currentVersion: string
  tokenConfigured: boolean
  lastCheckedAtUnix: number | null
  updateAvailable: boolean
  canStageUpdate: boolean
  canInstallUpdate: boolean
  feedKind: string
  latestVersion: string | null
  latestTitle: string | null
  latestPublishedAt: string | null
  latestCommitSha: string | null
  latestAssetName: string | null
  stagedAssetName: string | null
  stagedAssetPath: string | null
  stagedSha256: string | null
  stagedSha256Verified: boolean
  stagedAtUnix: number | null
  detail: string
  lastError: string | null
}

export type PerformanceLogEvent = {
  sessionId: string
  eventType: string
  occurredAtUnixMs: number
  activeTab: string
  detail: string
  payload: Record<string, unknown>
}

export async function getBackendBootstrap() {
  return invoke<BackendBootstrap>('get_backend_bootstrap')
}

export async function getHardwareReadinessSnapshot() {
  return invoke<HardwareReadinessSnapshot>('get_hardware_readiness_snapshot')
}

export async function repairHardwarePrerequisites() {
  return invoke<RepairLaunchResult>('repair_hardware_prerequisites')
}

export async function requestServiceHeal() {
  return invoke<ServiceHealResult>('request_service_heal')
}

export async function getBackendPollSnapshot() {
  return invoke<BackendPollSnapshot>('get_backend_poll_snapshot')
}

export async function getControlSnapshot() {
  return invoke<ControlSnapshot>('get_control_snapshot')
}

export async function getLiveControlSnapshot() {
  return invoke<LiveControlSnapshot>('get_live_control_snapshot')
}

export async function appendPerformanceLog(events: PerformanceLogEvent[]) {
  return invoke<string>('append_performance_log', { events })
}

export async function checkForUpdates(channel?: PersonalSettings['updateChannel']) {
  return invoke<UpdateStatus>('check_for_updates', { channel: channel ?? null })
}

export async function stageUpdateDownload(channel?: PersonalSettings['updateChannel']) {
  return invoke<UpdateStatus>('stage_update_download', { channel: channel ?? null })
}

export async function installStagedUpdate() {
  return invoke<UpdateStatus>('install_staged_update')
}

export async function showUpdateNotification(versionLabel: string) {
  return invoke<void>('show_update_notification', { versionLabel })
}

export async function showThermalWarningNotification(sensorLabel: string, tempC: number) {
  return invoke<void>('show_thermal_warning_notification', { sensorLabel, tempC })
}

export async function applyBlueLightFilter(enabled: boolean) {
  return invoke<BlueLightApplyResult>('apply_blue_light_filter', { enabled })
}

export async function applySmartCharging(enabled: boolean) {
  return invoke<SmartChargeApplyResult>('apply_smart_charging', { enabled })
}

export async function applyAutoRefreshRate(enabled: boolean, onBattery: boolean) {
  return invoke<DisplayRefreshApplyResult>('apply_auto_refresh_rate', { enabled, onBattery })
}

export async function setNvidiaTelemetryEnabled(enabled: boolean) {
  return invoke<NvidiaTelemetryApplyResult>('set_nvidia_telemetry_enabled', { enabled })
}

export async function saveControlSnapshot(snapshot: ControlSnapshot) {
  return invoke<ControlSnapshot>('save_control_snapshot', { snapshot })
}

export async function applyPowerProfile(
  profileId: PowerProfileId,
  processorState: ProcessorStateSettings,
  customBaseProfile?: CustomPowerBaseId | null,
  processorStateControlEnabled = true,
) {
  return invoke<ControlSnapshot>('apply_power_profile', {
    profileId,
    processorState,
    customBaseProfile: customBaseProfile ?? null,
    processorStateControlEnabled,
  })
}

export async function applyGpuTuning(tuning: GpuTuningState, activeOcSlot: string) {
  return invoke<GpuTuningApplyResult>('apply_gpu_tuning', { tuning, activeOcSlot })
}

export async function applyFanProfile(profileId: FanProfileId) {
  return invoke<FanControlApplyResult>('apply_fan_profile', { profileId })
}

export async function applyCustomFanCurves(curves: FanCurveSet) {
  return invoke<FanControlApplyResult>('apply_custom_fan_curves', { curves })
}

export async function startFanSpeedCalibration() {
  return invoke<FanSpeedCalibrationSnapshot>('start_fan_speed_calibration')
}

export async function cancelFanSpeedCalibration() {
  return invoke<FanSpeedCalibrationSnapshot>('cancel_fan_speed_calibration')
}

export async function applyBootLogo(
  fileName: string,
  imageBase64: string,
  selectedBootArt?: BootArtId,
) {
  return invoke<BootLogoApplyResult>('apply_boot_logo', {
    fileName,
    imageBase64,
    selectedBootArt,
  })
}
