import { useCallback, useEffect, useRef, useState } from 'react'
import type { AppToast } from '../components/ToastStack'

const DEFAULT_TOAST_MS = 4200
const MAX_VISIBLE_TOASTS = 4

/** Toast stack with per-toast auto-dismiss timers. Callbacks are referentially stable. */
export function useToasts() {
  const [toasts, setToasts] = useState<AppToast[]>([])
  const timersRef = useRef<Record<string, number>>({})
  const sequenceRef = useRef(0)

  const clearTimer = useCallback((id: string) => {
    const timer = timersRef.current[id]
    if (timer !== undefined) {
      window.clearTimeout(timer)
      delete timersRef.current[id]
    }
  }, [])

  const dismissToast = useCallback(
    (id: string) => {
      clearTimer(id)
      setToasts((current) => current.filter((toast) => toast.id !== id))
    },
    [clearTimer],
  )

  const dismissNewestToast = useCallback(() => {
    setToasts((current) => {
      if (current.length === 0) {
        return current
      }
      clearTimer(current[current.length - 1].id)
      return current.slice(0, -1)
    })
  }, [clearTimer])

  const showToast = useCallback(
    (tone: AppToast['tone'], title: string, detail: string, durationMs = DEFAULT_TOAST_MS) => {
      const id = `toast-${Date.now().toString(36)}-${sequenceRef.current++}`
      setToasts((current) => [...current.slice(-(MAX_VISIBLE_TOASTS - 1)), { id, tone, title, detail }])
      if (durationMs > 0) {
        timersRef.current[id] = window.setTimeout(() => dismissToast(id), durationMs)
      }
      return id
    },
    [dismissToast],
  )

  const updateToast = useCallback(
    (
      id: string,
      tone: AppToast['tone'],
      title: string,
      detail: string,
      durationMs = DEFAULT_TOAST_MS,
    ) => {
      clearTimer(id)
      setToasts((current) =>
        current.map((toast) => (toast.id === id ? { ...toast, tone, title, detail } : toast)),
      )
      if (durationMs > 0) {
        timersRef.current[id] = window.setTimeout(() => dismissToast(id), durationMs)
      }
    },
    [clearTimer, dismissToast],
  )

  useEffect(() => {
    const timers = timersRef.current
    return () => {
      Object.values(timers).forEach((timer) => window.clearTimeout(timer))
    }
  }, [])

  return { toasts, showToast, updateToast, dismissToast, dismissNewestToast }
}
