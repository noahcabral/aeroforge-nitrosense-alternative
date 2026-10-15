import aeroforgeMark from '../assets/aeroforge-mark.webp'
import type { BootArt } from '../lib/bootArt'

type BootSplashPreviewProps = {
  art: BootArt
  compact?: boolean
}

export function BootSplashPreview({ art, compact = false }: BootSplashPreviewProps) {
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

