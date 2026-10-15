import { useEffect, useRef, useState } from 'react'
import { formatDebugClock } from '../lib/format'
import { createFrameSampler, isSlowWindow, type FrameWindow } from '../lib/frameSampler'

export type FrameStats = FrameWindow & { updatedAt: string }

const IDLE_STATS: FrameStats = {
  averageMs: 0,
  maxMs: 0,
  fps: 0,
  longFrameCount: 0,
  sampleWindowMs: 0,
  updatedAt: 'Waiting',
}

/**
 * Drives a frame sampler from requestAnimationFrame while `enabled`.
 * `onSample` receives every completed window; `onSlowWindow` fires for janky
 * windows, throttled to one report per two seconds.
 */
export function useFrameStats(
  enabled: boolean,
  callbacks: { onSample: (stats: FrameStats) => void; onSlowWindow: (stats: FrameStats) => void },
) {
  const [frameStats, setFrameStats] = useState<FrameStats>(IDLE_STATS)
  const callbacksRef = useRef(callbacks)
  useEffect(() => {
    callbacksRef.current = callbacks
  })

  useEffect(() => {
    if (!enabled) {
      return
    }

    const sampler = createFrameSampler()
    let cancelled = false
    let rafId = 0
    let lastSlowReportAt = 0

    function step(timestamp: number) {
      if (cancelled) {
        return
      }
      const window_ = sampler.push(timestamp)
      if (window_) {
        const stats: FrameStats = { ...window_, updatedAt: formatDebugClock(new Date()) }
        setFrameStats(stats)
        callbacksRef.current.onSample(stats)
        if (isSlowWindow(window_) && timestamp - lastSlowReportAt >= 2000) {
          callbacksRef.current.onSlowWindow(stats)
          lastSlowReportAt = timestamp
        }
      }
      rafId = window.requestAnimationFrame(step)
    }

    rafId = window.requestAnimationFrame(step)
    return () => {
      cancelled = true
      window.cancelAnimationFrame(rafId)
    }
  }, [enabled])

  return frameStats
}
