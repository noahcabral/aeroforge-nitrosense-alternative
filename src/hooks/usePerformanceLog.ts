import { useCallback, useEffect, useRef, useState } from 'react'
import { appendPerformanceLog, type PerformanceLogEvent } from '../lib/backend'
import { describeError } from '../lib/errors'
import { formatDebugClock } from '../lib/format'

const BATCH_SIZE = 16
const FLUSH_DELAY_MS = 1500
const MAX_QUEUE = 200

export type PerformanceLogState = {
  path: string | null
  lastFlushAt: string
  pendingCount: number
  eventCount: number
  lastError: string | null
}

type Options = {
  /** Whether events should be recorded at all (desktop runtime + diagnostics on). */
  enabled: () => boolean
  /** Whether flushes may call the backend (desktop runtime). */
  canFlush: () => boolean
  activeTab: () => string
  onError: (message: string) => void
}

/**
 * Batched, size-capped event queue flushed to the backend performance log.
 * Options are read through a ref so the returned callbacks are stable.
 */
export function usePerformanceLog(options: Options) {
  const optionsRef = useRef(options)
  useEffect(() => {
    optionsRef.current = options
  })

  const [state, setState] = useState<PerformanceLogState>({
    path: null,
    lastFlushAt: 'Waiting',
    pendingCount: 0,
    eventCount: 0,
    lastError: null,
  })
  const sessionIdRef = useRef(`af-${Date.now().toString(36)}`)
  const queueRef = useRef<PerformanceLogEvent[]>([])
  const flushTimerRef = useRef<number | null>(null)
  const flushInFlightRef = useRef(false)
  const eventCountRef = useRef(0)
  const pathRef = useRef<string | null>(null)
  const flushRef = useRef<() => Promise<void>>(async () => {})

  const scheduleFlush = useCallback(() => {
    if (flushTimerRef.current !== null) {
      return
    }
    flushTimerRef.current = window.setTimeout(() => {
      flushTimerRef.current = null
      void flushRef.current()
    }, FLUSH_DELAY_MS)
  }, [])

  const flush = useCallback(async () => {
    if (!optionsRef.current.canFlush() || flushInFlightRef.current) {
      return
    }
    if (flushTimerRef.current !== null) {
      window.clearTimeout(flushTimerRef.current)
      flushTimerRef.current = null
    }

    const events = queueRef.current.splice(0)
    if (events.length === 0) {
      setState((current) => ({ ...current, pendingCount: 0, eventCount: eventCountRef.current }))
      return
    }

    flushInFlightRef.current = true
    try {
      const path = await appendPerformanceLog(events)
      pathRef.current = path
      setState({
        path,
        lastFlushAt: formatDebugClock(new Date()),
        pendingCount: queueRef.current.length,
        eventCount: eventCountRef.current,
        lastError: null,
      })
    } catch (error) {
      const message = describeError(error)
      setState({
        path: pathRef.current,
        lastFlushAt: formatDebugClock(new Date()),
        pendingCount: queueRef.current.length,
        eventCount: eventCountRef.current,
        lastError: message,
      })
      optionsRef.current.onError(`performance log write failed: ${message}`)
    } finally {
      flushInFlightRef.current = false
      if (queueRef.current.length > 0) {
        scheduleFlush()
      }
    }
  }, [scheduleFlush])
  flushRef.current = flush

  const queueEvent = useCallback(
    (eventType: string, detail: string, payload: Record<string, unknown> = {}) => {
      if (!optionsRef.current.enabled()) {
        return
      }

      queueRef.current.push({
        sessionId: sessionIdRef.current,
        eventType,
        occurredAtUnixMs: Date.now(),
        activeTab: optionsRef.current.activeTab(),
        detail,
        payload: { ...payload, visibility: document.visibilityState },
      })
      eventCountRef.current += 1

      if (queueRef.current.length > MAX_QUEUE) {
        queueRef.current.splice(0, queueRef.current.length - MAX_QUEUE)
      }

      if (queueRef.current.length >= BATCH_SIZE) {
        void flush()
      } else {
        scheduleFlush()
      }
    },
    [flush, scheduleFlush],
  )

  useEffect(() => {
    return () => {
      if (flushTimerRef.current !== null) {
        window.clearTimeout(flushTimerRef.current)
      }
    }
  }, [])

  return { state, queueEvent, flush }
}
