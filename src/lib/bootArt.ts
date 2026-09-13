/** Built-in boot splash artwork presets shared by the settings UI and boot-logo renderer. */

export type BootArt = {
  id: string
  name: string
  palette: string
  layout: 'forge' | 'center' | 'banner'
  headline: string
  subline: string
}
export const bootArtwork: BootArt[] = [
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
