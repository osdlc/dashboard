import vue from '@vitejs/plugin-vue'
import { defineConfig } from 'vite'

// In dev, run `dashboard serve --dev-origin http://localhost:5173` and open
// http://localhost:5173/?token=<token>. The proxy forwards API and WebSocket traffic.
export default defineConfig({
  plugins: [vue()],
  server: {
    port: 5173,
    strictPort: true,
    proxy: {
      '/api': { target: 'http://127.0.0.1:7878', ws: true },
    },
  },
})
