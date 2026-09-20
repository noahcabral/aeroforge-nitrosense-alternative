/** Boot-logo image preparation: renders preset artwork or re-encodes an upload for the firmware path. */

import aeroforgeMark from '../assets/aeroforge-mark.webp'
import aeroforgeWordmark from '../assets/aeroforge-wordmark.webp'
import type { BootArt } from './bootArt'

export type PreparedBootLogo = {
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

export function buildJpegBootLogoName(fileName: string) {
  return buildBootLogoName(fileName, 'jpg')
}

function buildBootLogoName(fileName: string, extension: 'jpg' | 'gif') {
  const stem = fileName.replace(/\.[^.]+$/, '').trim() || 'aeroforge-boot-logo'
  return `${stem}.${extension}`
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

export async function preparePresetBootLogo(art: BootArt): Promise<PreparedBootLogo> {
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

export async function prepareBootLogoUpload(file: File): Promise<PreparedBootLogo> {
  const sourceDataUrl = await readFileAsDataUrl(file)
  if (file.type === 'image/gif' || /\.gif$/i.test(file.name)) {
    return {
      fileName: buildBootLogoName(file.name, 'gif'),
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
