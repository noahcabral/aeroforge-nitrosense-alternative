import { readFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

const { version } = JSON.parse(readFileSync(new URL('./package.json', import.meta.url), 'utf8')) as {
  version: string
}

// Vite's dependency optimizer writes many small files with fd-based writes that
// cloud-synced folders (Google Drive streaming in particular) reject or stall on.
// Keep the cache on local disk; override with AEROFORGE_VITE_CACHE_DIR if needed.
const cacheDir = process.env.AEROFORGE_VITE_CACHE_DIR ?? join(tmpdir(), 'aeroforge-vite-cache')

// https://vite.dev/config/
export default defineConfig({
  plugins: [react()],
  cacheDir,
  clearScreen: false,
  define: {
    // Pre-bootstrap version label; the Tauri shell overwrites it once loaded.
    __APP_VERSION__: JSON.stringify(version),
  },
  server: {
    host: '127.0.0.1',
    port: 1420,
    strictPort: true,
    watch: {
      // The Rust crates and packaging folders hold tens of thousands of files the
      // UI never imports. Walking them stalls the dev server on slow filesystems,
      // and a Rust edit mid-session can crash the watcher with EINVAL on cloud-synced
      // mounts. Cargo watches the crates itself; Vite only needs src/ and public/.
      ignored: [
        '**/aeroforge-service/**',
        '**/src-tauri/**',
        '**/portable/**',
        '**/third_party/**',
        '**/AeroForge-Debug-*/**',
      ],
    },
  },
  envPrefix: ['VITE_', 'TAURI_'],
})
