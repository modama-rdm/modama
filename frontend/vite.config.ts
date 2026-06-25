import {defineConfig, loadEnv} from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'

// https://vite.dev/config/
export default defineConfig(({mode}) => {
    const env = loadEnv(mode, process.cwd(), '')

    // The backend can't run on macOS (Linux-only stack), so for local dev we
    // proxy the API to the test server — e.g. the UTM Debian VM running the test
    // stack. Set VITE_API_PROXY to that VM's address (default: the test stack's
    // mapped port). Never point this at production.
    const apiTarget = env.VITE_API_PROXY || 'http://localhost:5001'

    // Only the JSON API is proxied to the backend. The SPA owns its own /login,
    // /logout, etc. as client routes — proxying those would send the browser to
    // the legacy Flask login page instead of this app.
    const proxy = {
        '/api': {target: apiTarget, changeOrigin: true},
    }

    return {
        plugins: [react(), tailwindcss()],
        server: {port: 5180, proxy},
        test: {
            environment: 'jsdom',
            clearMocks: true,
        },
    }
})
