import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

export default defineConfig({
  plugins: [react()],
  // Served from cuas_api / Tauri webview at site root
  base: '/',
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    proxy: {
      '/ws': { target: 'ws://127.0.0.1:8080', ws: true },
      '/api': { target: 'http://127.0.0.1:8080' },
      '/health': { target: 'http://127.0.0.1:8080' },
    },
  },
})
