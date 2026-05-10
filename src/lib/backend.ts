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

export type CommandDescriptor = {
  command: string
  stage: string
  purpose: string
}

export type BackendContract = {
  schemaVersion: string
  commands: CommandDescriptor[]
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
}

export type PowerProfileId = 'battery-guard' | 'balanced' | 'performance' | 'turbo' | 'custom'
export type CustomPowerBaseId = 'balanced' | 'performance' | 'turbo'
export type FanProfileId = 'auto' | 'max' | 'custom'
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

export type PersonalSettings = {
  smartChargingEnabled: boolean
  usbPowerEnabled: boolean
  blueLightFilterEnabled: boolean
  autoRefreshRateOnBatteryEnabled: boolean
  autoRefreshRateRestoreHz: number | null
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
  fanSyncLockEnabled: boolean
  personalSettings: PersonalSettings
}

export type LiveControlSnapshot = {
  service: string
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
  activeFanProfile: FanProfileId | null
  activeFanCurves: FanCurveSet | null
  lastFanAppliedAtUnix: number | null
  lastFanApplyDetail: string
  lastFanError: string | null
  lastFanReadback: unknown | null
  bootLogoApplySupported: boolean
  lastBootLogoAppliedAtUnix: number | null
  lastBootLogoApplyDetail: string
  lastBootLogoError: string | null
  lastBootLogoReadback: unknown | null
}

export type TelemetrySnapshot = {
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

export type BackendBootstrap = {
  shell: ShellStatus
  service: ServiceStatus
  contract: BackendContract
  capabilities: CapabilitySnapshot
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

export async function getRuntimeShell() {
  return invoke<ShellStatus>('runtime_shell')
}

export async function getBackendBootstrap() {
  return invoke<BackendBootstrap>('get_backend_bootstrap')
}

export async function getServiceStatus() {
  return invoke<ServiceStatus>('get_service_status')
}

export async function getTelemetrySnapshot() {
  return invoke<TelemetrySnapshot>('get_telemetry_snapshot')
}

export async function getBackendPollSnapshot() {
  return invoke<BackendPollSnapshot>('get_backend_poll_snapshot')
}

export async function getLiveControlSnapshot() {
  return invoke<LiveControlSnapshot>('get_live_control_snapshot')
}

export async function getPersistenceStatus() {
  return invoke<PersistenceStatus>('get_persistence_status')
}

export async function getUpdateStatus() {
  return invoke<UpdateStatus>('get_update_status')
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

export async function applyBlueLightFilter(enabled: boolean) {
  return invoke<BlueLightApplyResult>('apply_blue_light_filter', { enabled })
}

export async function applySmartCharging(enabled: boolean) {
  return invoke<SmartChargeApplyResult>('apply_smart_charging', { enabled })
}

export async function applyAutoRefreshRate(enabled: boolean, onBattery: boolean) {
  return invoke<DisplayRefreshApplyResult>('apply_auto_refresh_rate', { enabled, onBattery })
}

export async function saveControlSnapshot(snapshot: ControlSnapshot) {
  return invoke<ControlSnapshot>('save_control_snapshot', { snapshot })
}

export async function resetControlSnapshot() {
  return invoke<ControlSnapshot>('reset_control_snapshot')
}

export async function applyPowerProfile(
  profileId: PowerProfileId,
  processorState: ProcessorStateSettings,
  customBaseProfile?: CustomPowerBaseId | null,
) {
  return invoke<ControlSnapshot>('apply_power_profile', {
    profileId,
    processorState,
    customBaseProfile: customBaseProfile ?? null,
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
