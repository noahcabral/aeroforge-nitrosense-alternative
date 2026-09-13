/** Shared error formatting for UI status lines and toasts. */

export function describeError(error: unknown): string {
  if (error instanceof Error) {
    return error.message
  }

  if (typeof error === 'string') {
    return error
  }

  try {
    return JSON.stringify(error)
  } catch {
    return 'Unknown error'
  }
}

export function withTimeout<T>(
  promise: Promise<T>,
  timeoutMs: number,
  label: string,
): Promise<T> {
  let timeoutId: ReturnType<typeof setTimeout> | null = null

  const timeout = new Promise<T>((_, reject) => {
    timeoutId = setTimeout(() => {
      reject(new Error(`${label} timed out after ${Math.round(timeoutMs / 1000)} seconds`))
    }, timeoutMs)
  })

  return Promise.race([promise, timeout]).finally(() => {
    if (timeoutId !== null) {
      clearTimeout(timeoutId)
    }
  })
}
