import { useCallback, useEffect, useRef, useState } from 'react'
import { formatDebugClock } from '../lib/format'

const MAX_ENTRIES = 8
const HEARTBEAT_INTERVAL_MS = 5000

export type DebugLogEntry = { id: number; text: string }

type Options = {
  /** Mirror entries to console.debug (dev builds / debug tab). */
  echoToConsole: () => boolean
  /** Only re-render when the log is actually on screen. */
  isVisible: () => boolean
}

/**
 * Small ring of timestamped debug lines. Entries are kept in a ref and only
 * pushed into React state while visible; call `reveal` when the log comes
 * into view. Returned callbacks are stable.
 */
export function useDebugLog(options: Options) {
  const optionsRef = useRef(options)
  useEffect(() => {
    optionsRef.current = options
  })

  const [events, setEvents] = useState<DebugLogEntry[]>([])
  const entriesRef = useRef<DebugLogEntry[]>([])
  const nextIdRef = useRef(0)
  const lastTransportRef = useRef('')
  const lastHeartbeatAtRef = useRef(0)

  const push = useCallback((message: string) => {
    const text = `${formatDebugClock(new Date())} ${message}`
    if (optionsRef.current.echoToConsole()) {
      console.debug(`[AeroForge debug] ${text}`)
    }
    // Identical messages within one second are common (retries); a unique id
    // keeps React keys unique so a stale row is never left behind.
    const entry = { id: nextIdRef.current++, text }
    entriesRef.current = [entry, ...entriesRef.current].slice(0, MAX_ENTRIES)
    if (optionsRef.current.isVisible()) {
      setEvents(entriesRef.current)
    }
  }, [])

  /** De-duplicated: repeated identical transport messages are logged once. */
  const pushTransport = useCallback(
    (message: string) => {
      if (lastTransportRef.current === message) {
        return
      }
      lastTransportRef.current = message
      push(message)
    },
    [push],
  )

  /** Rate-limited to one entry per five seconds. */
  const pushHeartbeat = useCallback(
    (message: string) => {
      const now = Date.now()
      if (now - lastHeartbeatAtRef.current < HEARTBEAT_INTERVAL_MS) {
        return
      }
      lastHeartbeatAtRef.current = now
      push(message)
    },
    [push],
  )

  const clear = useCallback(() => {
    entriesRef.current = [
      { id: nextIdRef.current++, text: `${formatDebugClock(new Date())} debug event log cleared` },
    ]
    setEvents(entriesRef.current)
  }, [])

  const reveal = useCallback(() => setEvents([...entriesRef.current]), [])
  const snapshot = useCallback(() => entriesRef.current.map((entry) => entry.text), [])

  return { events, push, pushTransport, pushHeartbeat, clear, reveal, snapshot }
}
