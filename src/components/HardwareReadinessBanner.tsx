import type { HardwareReadinessSnapshot } from '../lib/backend'

type HardwareReadinessBannerProps = {
  title: string
  detail: string
  readiness: HardwareReadinessSnapshot
  repairPending: boolean
  startPending: boolean
  onRepair: () => void
  onStartService: () => void
}

export function HardwareReadinessBanner({
  title,
  detail,
  readiness,
  repairPending,
  startPending,
  onRepair,
  onStartService,
}: HardwareReadinessBannerProps) {
  const model = readiness.acer.systemModel ?? 'Unknown Acer model'
  const serviceState = readiness.service.installed
    ? readiness.service.running
      ? 'Service running'
      : 'Service stopped'
    : 'Service missing'
  const wmiState = readiness.wmi.disabled
    ? 'WMI disabled'
    : readiness.wmi.service.running
      ? 'WMI running'
      : 'WMI stopped'

  return (
    <section className="hardware-banner">
      <div>
        <span className="eyebrow">Hardware Readiness</span>
        <strong>{title}</strong>
        <p>{detail}</p>
      </div>
      <div className="hardware-banner__facts">
        <span>{model}</span>
        <span>{serviceState}</span>
        <span>{wmiState}</span>
      </div>
      <div className="hardware-banner__actions">
        {readiness.service.installed && !readiness.service.running && (
          <button
            className="button button--primary"
            disabled={startPending || repairPending}
            onClick={onStartService}
          >
            {startPending ? 'Starting In Background...' : 'Start In Background'}
          </button>
        )}
        <button
          className={
            readiness.service.installed && !readiness.service.running
              ? 'button button--ghost'
              : 'button button--primary'
          }
          disabled={!readiness.repairAvailable || repairPending || startPending}
          onClick={onRepair}
        >
          {repairPending ? 'Launching Repair...' : 'Repair Service And WMI'}
        </button>
      </div>
    </section>
  )
}
