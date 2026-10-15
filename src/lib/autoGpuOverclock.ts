/**
 * Auto GPU overclock finder for AeroForge.
 *
 * This does not run a stress harness (no artifact/crash detection in-process).
 * It picks the best *safe* NitroSense-matched tier from live conditions and
 * returns verification instructions the user should follow after apply.
 *
 * Bounds stay inside the product's existing NVML clamps and NitroSense presets:
 * stock 0/0, mild +50/+100, NitroSense +100/+200. Voltage/power%/temp are never
 * recommended as live writes.
 */

export type AutoGpuTierId = 'stock' | 'mild' | 'nitrosense'

export type AutoGpuTuningOffsets = {
  coreClockMhz: number
  memoryClockMhz: number
}

export type AutoGpuFinderInput = {
  acPluggedIn: boolean | null
  gpuTempC: number | null
  cpuTempC: number | null
  systemTempC: number | null
  gpuName: string | null
  nvidiaTelemetryEnabled: boolean
  ocTuningLocked: boolean
  /** Optional model-specific memory offset (ANV15-51 may use 0). */
  nitrosenseMemoryOffsetMhz?: number
}

export type AutoGpuFinderResult = {
  tierId: AutoGpuTierId
  tierLabel: string
  offsets: AutoGpuTuningOffsets
  /** Why this tier was chosen. */
  summary: string
  /** Blocking reason when apply should not proceed. */
  blockedReason: string | null
  canApply: boolean
  /** Ordered checklist shown in the UI after a find/apply. */
  instructions: string[]
  /** Suggested OC slot id when applying a built-in preset. */
  suggestedOcSlotId: 'silent-uv' | 'daily' | 'creator' | 'arena'
}

const THERMAL_BLOCK_C = 88
const THERMAL_STOCK_ONLY_C = 82
const THERMAL_MILD_MAX_C = 78

export const AUTO_GPU_TIERS: Record<
  AutoGpuTierId,
  {
    label: string
    ocSlotId: AutoGpuFinderResult['suggestedOcSlotId']
    offsets: (memoryOffset: number) => AutoGpuTuningOffsets
    blurb: string
  }
> = {
  stock: {
    label: 'Stock',
    ocSlotId: 'daily',
    offsets: () => ({ coreClockMhz: 0, memoryClockMhz: 0 }),
    blurb: 'Driver-default clocks. Safest baseline.',
  },
  mild: {
    label: 'Mild',
    ocSlotId: 'creator',
    offsets: () => ({ coreClockMhz: 50, memoryClockMhz: 100 }),
    blurb: 'Conservative step below NitroSense Extreme.',
  },
  nitrosense: {
    label: 'NitroSense Extreme',
    ocSlotId: 'arena',
    offsets: (memoryOffset) => ({
      coreClockMhz: 100,
      memoryClockMhz: memoryOffset,
    }),
    blurb: 'Matches Acer NitroSense Performance/Turbo GPU offsets.',
  },
}

function hottestTempC(input: AutoGpuFinderInput): number | null {
  const values = [input.gpuTempC, input.cpuTempC, input.systemTempC].filter(
    (value): value is number => typeof value === 'number' && Number.isFinite(value),
  )
  if (values.length === 0) {
    return null
  }
  return Math.max(...values)
}

function baseInstructions(tier: AutoGpuTierId, offsets: AutoGpuTuningOffsets): string[] {
  const core =
    offsets.coreClockMhz >= 0 ? `+${offsets.coreClockMhz}` : `${offsets.coreClockMhz}`
  const mem =
    offsets.memoryClockMhz >= 0
      ? `+${offsets.memoryClockMhz}`
      : `${offsets.memoryClockMhz}`

  const shared = [
    `Target: ${AUTO_GPU_TIERS[tier].label} (core ${core} MHz / memory ${mem} MHz).`,
    'Stay on AC power. Battery sessions should use stock clocks.',
    'Enable NVIDIA Telemetry in Settings if live apply fails or GPU readouts stay blank.',
    'Apply Live, then run a familiar game or 3D workload for 10–15 minutes.',
    'Watch for flicker, artifacts, driver resets, or hard freezes. If any appear, step down one tier and retest.',
    'If GPU stays above ~85°C for long stretches, prefer Mild or Stock and raise fan profile toward Cool/Max.',
    'Voltage, power %, and temp limit stay staged only — AeroForge does not write those on this path.',
  ]

  if (tier === 'stock') {
    return [
      ...shared,
      'Stock is the recovery tier: use it after instability, on battery, or when thermals are already high.',
    ]
  }

  if (tier === 'mild') {
    return [
      ...shared,
      'After Mild is stable, you can re-run Find Best Settings when idle and cool to try NitroSense Extreme.',
    ]
  }

  return [
    ...shared,
    'NitroSense Extreme is the highest AeroForge auto tier. Do not push sliders past this without your own validation.',
    'Save To Custom once the session stays clean so the offsets survive profile changes you make later.',
  ]
}

/**
 * Recommend the best safe GPU OC tier from current conditions.
 */
export function findBestGpuOverclock(input: AutoGpuFinderInput): AutoGpuFinderResult {
  const memoryOffset = input.nitrosenseMemoryOffsetMhz ?? 200
  const hottest = hottestTempC(input)

  if (input.ocTuningLocked) {
    const offsets = AUTO_GPU_TIERS.stock.offsets(memoryOffset)
    return {
      tierId: 'stock',
      tierLabel: AUTO_GPU_TIERS.stock.label,
      offsets,
      summary: 'Tuner is locked. Unlock it before Auto GPU Overclocking can change offsets.',
      blockedReason: 'Unlock the GPU tuner first.',
      canApply: false,
      instructions: baseInstructions('stock', offsets),
      suggestedOcSlotId: 'daily',
    }
  }

  if (input.acPluggedIn === false) {
    const offsets = AUTO_GPU_TIERS.stock.offsets(memoryOffset)
    return {
      tierId: 'stock',
      tierLabel: AUTO_GPU_TIERS.stock.label,
      offsets,
      summary: 'On battery: Auto GPU Overclocking keeps stock clocks only.',
      blockedReason: null,
      canApply: true,
      instructions: [
        ...baseInstructions('stock', offsets),
        'Plug into AC, let the laptop cool, then run Find Best Settings again for Mild or NitroSense Extreme.',
      ],
      suggestedOcSlotId: 'daily',
    }
  }

  if (!input.nvidiaTelemetryEnabled) {
    const offsets = AUTO_GPU_TIERS.stock.offsets(memoryOffset)
    return {
      tierId: 'stock',
      tierLabel: AUTO_GPU_TIERS.stock.label,
      offsets,
      summary:
        'NVIDIA Telemetry is off, so live GPU offset apply may be skipped. Turn it on in Settings → Battery & Charge, then find again.',
      blockedReason: 'Enable NVIDIA Telemetry to apply live GPU offsets.',
      canApply: false,
      instructions: [
        'Open Settings → Battery & Charge.',
        'Enable NVIDIA Telemetry Polling.',
        'Return here and press Find Best Settings.',
        'Stay on AC power with cool idle temperatures before applying a positive offset.',
      ],
      suggestedOcSlotId: 'daily',
    }
  }

  if (hottest != null && hottest >= THERMAL_BLOCK_C) {
    const offsets = AUTO_GPU_TIERS.stock.offsets(memoryOffset)
    return {
      tierId: 'stock',
      tierLabel: AUTO_GPU_TIERS.stock.label,
      offsets,
      summary: `Thermals are too high (${Math.round(hottest)}°C). Auto GPU Overclocking recommends stock until the laptop cools.`,
      blockedReason: null,
      canApply: true,
      instructions: [
        ...baseInstructions('stock', offsets),
        'Switch fans to Cool or Max, close heavy apps, wait until GPU is under ~75°C, then find again.',
      ],
      suggestedOcSlotId: 'silent-uv',
    }
  }

  if (hottest != null && hottest >= THERMAL_STOCK_ONLY_C) {
    const offsets = AUTO_GPU_TIERS.stock.offsets(memoryOffset)
    return {
      tierId: 'stock',
      tierLabel: AUTO_GPU_TIERS.stock.label,
      offsets,
      summary: `Warm package (${Math.round(hottest)}°C): staying on stock. Cool below ${THERMAL_STOCK_ONLY_C}°C before Mild/Extreme.`,
      blockedReason: null,
      canApply: true,
      instructions: baseInstructions('stock', offsets),
      suggestedOcSlotId: 'daily',
    }
  }

  if (hottest != null && hottest >= THERMAL_MILD_MAX_C) {
    const offsets = AUTO_GPU_TIERS.mild.offsets(memoryOffset)
    return {
      tierId: 'mild',
      tierLabel: AUTO_GPU_TIERS.mild.label,
      offsets,
      summary: `Moderate heat (${Math.round(hottest)}°C): Mild (+50 / +100) is the best safe auto pick right now.`,
      blockedReason: null,
      canApply: true,
      instructions: baseInstructions('mild', offsets),
      suggestedOcSlotId: 'creator',
    }
  }

  // Cool / unknown temps on AC with telemetry → NitroSense Extreme.
  const offsets = AUTO_GPU_TIERS.nitrosense.offsets(memoryOffset)
  const gpuLabel = input.gpuName?.trim() || 'this NVIDIA GPU'
  const tempNote =
    hottest == null
      ? 'No live temperature yet; recommending NitroSense Extreme for AC + telemetry ready.'
      : `Cool package (${Math.round(hottest)}°C): NitroSense Extreme is the best safe auto pick for ${gpuLabel}.`

  return {
    tierId: 'nitrosense',
    tierLabel: AUTO_GPU_TIERS.nitrosense.label,
    offsets,
    summary: tempNote,
    blockedReason: null,
    canApply: true,
    instructions: baseInstructions('nitrosense', offsets),
    suggestedOcSlotId: 'arena',
  }
}

/** Step down one tier after the user reports instability. */
export function stepDownGpuOverclockTier(tier: AutoGpuTierId): AutoGpuTierId {
  if (tier === 'nitrosense') {
    return 'mild'
  }
  return 'stock'
}

export function resultForTier(
  tier: AutoGpuTierId,
  input: Pick<AutoGpuFinderInput, 'nitrosenseMemoryOffsetMhz'>,
  summary: string,
): AutoGpuFinderResult {
  const memoryOffset = input.nitrosenseMemoryOffsetMhz ?? 200
  const meta = AUTO_GPU_TIERS[tier]
  const offsets = meta.offsets(memoryOffset)
  return {
    tierId: tier,
    tierLabel: meta.label,
    offsets,
    summary,
    blockedReason: null,
    canApply: true,
    instructions: baseInstructions(tier, offsets),
    suggestedOcSlotId: meta.ocSlotId,
  }
}
