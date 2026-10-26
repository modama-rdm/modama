import {readFileSync} from 'node:fs'
import {defineConfig, loadEnv} from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'

// Single source of truth for the app version: package.json. Read it here and
// inject it as the `__APP_VERSION__` global (see `define` below) so the UI never
// keeps a second copy that can drift. Bump `version` in package.json in the same
// commit as a feature and add a matching CHANGELOG.md entry.
const pkg = JSON.parse(readFileSync('./package.json', 'utf-8')) as {version: string}

// https://vite.dev/config/
export default defineConfig(({mode}) => {
    const env = loadEnv(mode, process.cwd(), '')

    // The backend can't run on macOS (Linux-only stack), so for local dev we
    // proxy the API to the test server — e.g. the UTM Debian VM running the test
    // stack. Set VITE_API_PROXY to that VM's address (default: the test stack's
    // mapped port). Never point this at production.
    const apiTarget = env.VITE_API_PROXY || 'http://localhost:5001'

    // Only the JSON API is proxied to the backend: /mgmt-api, the FastAPI
    // service. The SPA owns its own /login, /logout, etc. as client routes —
    // proxying those would send the browser to the legacy Flask login page
    // instead of this app.
    const proxy = {
        '/mgmt-api': {target: apiTarget, changeOrigin: true},
    }

    // Public base path the app is served from. In production the new UI lives at
    // a sub-path under the mgmt domain (the legacy Flask UI keeps the root), so
    // the Docker build sets VITE_BASE_PATH=/new/. Local dev keeps the root ('/')
    // so the app stays at http://localhost:5180/. The router reads this same
    // value back via import.meta.env.BASE_URL for its basename — keep them in
    // sync by never hardcoding the path anywhere else.
    const base = env.VITE_BASE_PATH || '/'

    // Serve static assets (logos, favicon) straight from the repo's shared
    // top-level branding/ folder instead of keeping a duplicate copy in the app.
    // Vite publishes everything here at the base root, so `branding/modama.svg`
    // is referenced as `${BASE_URL}modama.svg`. The Docker build makes this path
    // resolve by building from the repo root (see Dockerfile).
    const publicDir = '../branding'

    return {
        base,
        publicDir,
        define: {__APP_VERSION__: JSON.stringify(pkg.version)},
        plugins: [react(), tailwindcss()],
        server: {port: 5180, proxy},
        test: {
            environment: 'jsdom',
            clearMocks: true,
            unstubGlobals: true,
            setupFiles: ['./src/test/setup.ts'],
        },
    }
})
