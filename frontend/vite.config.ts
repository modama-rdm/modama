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

    // Public base path the app is served from. In production the new UI lives at
    // a sub-path under the mgmt domain (the legacy Flask UI keeps the root), so
    // the Docker build sets VITE_BASE_PATH=/new/. Local dev keeps the root ('/')
    // so the app stays at http://localhost:5180/. The router reads this same
    // value back via import.meta.env.BASE_URL for its basename — keep them in
    // sync by never hardcoding the path anywhere else.
    const base = env.VITE_BASE_PATH || '/'

    return {
        base,
        plugins: [react(), tailwindcss()],
        server: {port: 5180, proxy},
        test: {
            environment: 'jsdom',
            clearMocks: true,
        },
    }
})
