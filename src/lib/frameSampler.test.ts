import { describe, expect, it } from 'vitest'
import { createFrameSampler, isSlowWindow } from './frameSampler'

function feed(sampler: ReturnType<typeof createFrameSampler>, deltas: number[], start = 1000) {
  const windows = []
  let t = start
  windows.push(sampler.push(t))
  for (const delta of deltas) {
    t += delta
    windows.push(sampler.push(t))
  }
  return windows.filter((w) => w !== null)
}

describe('createFrameSampler', () => {
  it('emits one window per second with average, max and fps', () => {
    const sampler = createFrameSampler()
    const windows = feed(sampler, Array(63).fill(16))
    expect(windows).toHaveLength(1)
    expect(windows[0].averageMs).toBeCloseTo(16)
    expect(windows[0].maxMs).toBe(16)
    expect(windows[0].fps).toBeCloseTo(62.5)
    expect(windows[0].longFrameCount).toBe(0)
    expect(windows[0].sampleWindowMs).toBe(63 * 16)
  })

  it('counts long frames and resets between windows', () => {
    const sampler = createFrameSampler()
    const first = feed(sampler, [16, 16, 60, 16, 900])
    expect(first).toHaveLength(1)
    expect(first[0].longFrameCount).toBe(2)
    expect(first[0].maxMs).toBe(900)

    // Next window starts clean: continue from the last timestamp (1000 + 16+16+60+16+900).
    const last = 1000 + 16 + 16 + 60 + 16 + 900
    let t = last
    const second = [16, 16, 990].map((delta) => sampler.push((t += delta))).filter((w) => w !== null)
    expect(second).toHaveLength(1)
    expect(second[0].longFrameCount).toBe(1)
    expect(second[0].maxMs).toBe(990)
  })

  it('returns null until a full window has elapsed', () => {
    const sampler = createFrameSampler()
    expect(sampler.push(0)).toBeNull()
    expect(sampler.push(16)).toBeNull()
    expect(sampler.push(999)).toBeNull()
    expect(sampler.push(1000)).not.toBeNull()
  })
})

describe('isSlowWindow', () => {
  const base = { averageMs: 16, maxMs: 20, fps: 60, longFrameCount: 0, sampleWindowMs: 1000 }
  it('flags a single very long frame, repeated long frames, or a slow average', () => {
    expect(isSlowWindow(base)).toBe(false)
    expect(isSlowWindow({ ...base, maxMs: 50 })).toBe(true)
    expect(isSlowWindow({ ...base, longFrameCount: 3 })).toBe(true)
    expect(isSlowWindow({ ...base, averageMs: 20 })).toBe(true)
  })
})
