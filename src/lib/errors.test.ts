import { describe, expect, it, vi } from 'vitest'
import { describeError, withTimeout } from './errors'

describe('describeError', () => {
  it('uses the message of Error instances', () => {
    expect(describeError(new Error('boom'))).toBe('boom')
  })

  it('passes strings through', () => {
    expect(describeError('plain')).toBe('plain')
  })

  it('serializes other values as JSON', () => {
    expect(describeError({ code: 3 })).toBe('{"code":3}')
    expect(describeError(42)).toBe('42')
  })

  it('falls back when the value cannot be serialized', () => {
    const cyclic: Record<string, unknown> = {}
    cyclic.self = cyclic
    expect(describeError(cyclic)).toBe('Unknown error')
  })
})

describe('withTimeout', () => {
  it('resolves with the inner value when it settles in time', async () => {
    await expect(withTimeout(Promise.resolve('ok'), 1000, 'op')).resolves.toBe('ok')
  })

  it('propagates inner rejections', async () => {
    await expect(withTimeout(Promise.reject(new Error('inner')), 1000, 'op')).rejects.toThrow(
      'inner',
    )
  })

  it('rejects with a labelled message once the deadline passes', async () => {
    vi.useFakeTimers()
    try {
      const pending = withTimeout(new Promise<never>(() => {}), 15_000, 'Fan apply')
      const assertion = expect(pending).rejects.toThrow('Fan apply timed out after 15 seconds')
      await vi.advanceTimersByTimeAsync(15_000)
      await assertion
    } finally {
      vi.useRealTimers()
    }
  })

  it('clears its timer after the inner promise settles', async () => {
    vi.useFakeTimers()
    try {
      await withTimeout(Promise.resolve(1), 5000, 'op')
      expect(vi.getTimerCount()).toBe(0)
    } finally {
      vi.useRealTimers()
    }
  })
})
