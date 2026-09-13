import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { defineConfig } from 'vitest/config'

export default defineConfig({
  // Same reason as vite.config.ts: keep transform caches off cloud-synced folders.
  cacheDir: process.env.AEROFORGE_VITE_CACHE_DIR ?? join(tmpdir(), 'aeroforge-vite-cache'),
  define: {
    __APP_VERSION__: JSON.stringify('test'),
  },
  test: {
    include: ['src/**/*.test.ts'],
    environment: 'node',
  },
})
