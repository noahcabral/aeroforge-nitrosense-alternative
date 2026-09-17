/** Frames at or above this are counted as "long" (two 60 Hz frames). */
export const LONG_FRAME_MS = 34

export type FrameWindow = {
  averageMs: number
  maxMs: number
  fps: number
  longFrameCount: number
  sampleWindowMs: number
}

/**
 * Accumulates requestAnimationFrame timestamps into fixed-length windows.
 * `push` returns a completed window once `windowMs` has elapsed, else null.
 */
export function createFrameSampler(windowMs = 1000, longFrameMs = LONG_FRAME_MS) {
  let previousTimestamp: number | null = null
  let windowStartedAt = 0
  let frameCount = 0
  let totalDeltaMs = 0
  let maxDeltaMs = 0
  let longFrameCount = 0

  return {
    push(timestamp: number): FrameWindow | null {
      if (previousTimestamp === null) {
        previousTimestamp = timestamp
        windowStartedAt = timestamp
        return null
      }

      const deltaMs = timestamp - previousTimestamp
      previousTimestamp = timestamp
      frameCount += 1
      totalDeltaMs += deltaMs
      maxDeltaMs = Math.max(maxDeltaMs, deltaMs)
      if (deltaMs >= longFrameMs) {
        longFrameCount += 1
      }

      const sampleWindowMs = timestamp - windowStartedAt
      if (sampleWindowMs < windowMs) {
        return null
      }

      const averageMs = frameCount > 0 ? totalDeltaMs / frameCount : 0
      const completed: FrameWindow = {
        averageMs,
        maxMs: maxDeltaMs,
        fps: averageMs > 0 ? 1000 / averageMs : 0,
        longFrameCount,
        sampleWindowMs,
      }
      windowStartedAt = timestamp
      frameCount = 0
      totalDeltaMs = 0
      maxDeltaMs = 0
      longFrameCount = 0
      return completed
    },
  }
}

/** A window worth surfacing in the debug log. */
export function isSlowWindow(window: FrameWindow) {
  return window.maxMs >= 50 || window.longFrameCount >= 3 || window.averageMs >= 20
}
