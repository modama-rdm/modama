import { defineConfig, loadEnv } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'

// https://vite.dev/config/
export default defineConfig(({ mode }) => {
  const env = loadEnv(mode, process.cwd(), '')

  // The backend can't run on macOS (Linux-only stack), so for local dev we
  // proxy the API to the test server — e.g. the UTM Debian VM running the test
  // stack. Set VITE_API_PROXY to that VM's address (default: the test stack's
  // mapped port). Never point this at production.
  const apiTarget = env.VITE_API_PROXY || 'http://localhost:5001'

  // Routes still owned by the legacy Flask app are proxied through so the SPA
  // can co-exist with the server-rendered pages during the migration.
  const proxied = ['/api', '/login', '/logout', '/browse', '/static']
  const proxy = Object.fromEntries(
    proxied.map((path) => [path, { target: apiTarget, changeOrigin: true }]),
  )

  return {
    plugins: [react(), tailwindcss()],
    server: { proxy },
  }
})
