/**
 * The seam between the journey tests and the backend.
 *
 * Journeys describe *what the world contains* (these projects, these members) and
 * never touch `fetch` themselves. Today that world is served from frozen fixtures
 * captured off the real API; when the FastAPI rewrite (#172) lands, the journeys
 * and their assertions stay as they are and only this file plus the fixture JSON
 * change — or, at cutover, `installApiMock` is simply not called and the same
 * journeys run against a live server.
 *
 * Handlers match on method + pathname. Query strings are exposed to the handler
 * rather than matched on, so `/api/browse/?path=…` stays one handler.
 */

import {vi} from 'vitest'

export interface ApiRequest {
    method: string
    /** Pathname only, e.g. `/api/project/foo/`. */
    path: string
    params: URLSearchParams
    /** Parsed JSON body, or undefined for bodyless requests. */
    body: unknown
}

export interface ApiReply {
    /** Defaults to 200. Use 204 for a bodiless success (the body is then ignored). */
    status?: number
    body: unknown
}

export interface ApiHandler {
    method: 'GET' | 'POST' | 'PUT' | 'DELETE' | 'PATCH'
    /** Exact pathname, or a pattern with `:param` segments (`/api/project/:name/`). */
    path: string
    respond: (req: ApiRequest) => ApiReply | Promise<ApiReply>
}

/** Every request the app made, in order — for asserting on mutations. */
export interface ApiRecorder {
    calls: ApiRequest[]
    /** Calls matching a method and exact pathname. */
    matching(method: string, path: string): ApiRequest[]
}

/** `/api/project/:name/` -> a regex capturing `name`. */
function compile(path: string): {re: RegExp; keys: string[]} {
    const keys: string[] = []
    const source = path
        .split('/')
        .map(seg => {
            if (!seg.startsWith(':')) return seg.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
            keys.push(seg.slice(1))
            return '([^/]+)'
        })
        .join('/')
    return {re: new RegExp(`^${source}$`), keys}
}

/**
 * Replace global `fetch` with one that answers from `handlers`. Vitest's
 * `clearMocks` plus the `afterEach` restore in `setup.ts` undo this per test.
 *
 * First match wins. To override one of `baseWorld()`'s GET handlers for a
 * single test, list your override *before* the `...baseWorld()` spread —
 * `[myOverride, ...baseWorld()]`, not the other way round.
 *
 * An unhandled request throws rather than returning a 404: a journey hitting an
 * endpoint nobody stubbed is a gap in the fixtures, and it should be loud.
 */
export function installApiMock(handlers: ApiHandler[]): ApiRecorder {
    const compiled = handlers.map(h => ({...h, ...compile(h.path)}))
    const calls: ApiRequest[] = []

    vi.stubGlobal('fetch', async (input: RequestInfo | URL, init: RequestInit = {}) => {
        const raw = typeof input === 'string' ? input : input.toString()
        const url = new URL(raw, 'http://localhost')
        const method = (init.method ?? 'GET').toUpperCase()

        let body: unknown
        if (typeof init.body === 'string') {
            try {
                body = JSON.parse(init.body)
            } catch {
                body = init.body
            }
        }

        const req: ApiRequest = {method, path: url.pathname, params: url.searchParams, body}
        calls.push(req)

        for (const h of compiled) {
            if (h.method !== method) continue
            if (!h.re.test(url.pathname)) continue
            const reply = await h.respond(req)
            const status = reply.status ?? 200
            // A 204 may not carry a body; `Response` throws if given one.
            if (status === 204) return new Response(null, {status})
            return new Response(JSON.stringify(reply.body), {
                status,
                headers: {'Content-Type': 'application/json'},
            })
        }

        throw new Error(
            `No API fixture for ${method} ${url.pathname}${url.search} — ` +
            `add a handler in the journey's world().`,
        )
    })

    return {
        calls,
        matching: (method, path) =>
            calls.filter(c => c.method === method.toUpperCase() && c.path === path),
    }
}

/** Pull a `:param` value back out of a request path (e.g. the project name). */
export function paramFrom(pattern: string, path: string, key: string): string {
    const {re, keys} = compile(pattern)
    const m = re.exec(path)
    const i = keys.indexOf(key)
    return m && i >= 0 ? decodeURIComponent(m[i + 1]) : ''
}
