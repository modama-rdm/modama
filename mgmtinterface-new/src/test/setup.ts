import '@testing-library/jest-dom/vitest'
import {cleanup} from '@testing-library/react'
import {afterEach} from 'vitest'

/**
 * Provide `localStorage` in the test environment.
 *
 * jsdom implements it (a plain `new JSDOM()` has it), but under vitest's jsdom
 * environment `window.localStorage` is an own, non-enumerable accessor that
 * returns undefined — while `sessionStorage` comes through normally. The likely
 * culprit is Node's experimental Web Storage global shadowing jsdom's, but the
 * shim is the same either way. Without it `useTheme` throws on first render and
 * takes every screen behind `AppShell` down with it.
 */
if (!window.localStorage) {
    const store = new Map<string, string>()
    Object.defineProperty(window, 'localStorage', {
        configurable: true,
        value: {
            getItem: (k: string) => store.get(k) ?? null,
            setItem: (k: string, v: string) => void store.set(k, String(v)),
            removeItem: (k: string) => void store.delete(k),
            clear: () => store.clear(),
            key: (i: number) => [...store.keys()][i] ?? null,
            get length() {
                return store.size
            },
        } satisfies Storage,
    })
}

/**
 * Replace `Request` with a minimal shim for every client-side navigation.
 *
 * Vitest's jsdom environment ends up with Node's real, strict `Request` class
 * on `window` (copied in verbatim) alongside jsdom's own from-scratch
 * `AbortController`/`AbortSignal` (never touched by that copy — vitest's inject
 * list omits them). React Router's data-mode router builds a real `Request` per
 * navigation with `signal: new AbortController().signal` for loader
 * cancellation; Node's `Request` rejects that signal with "Expected signal to
 * be an instance of AbortSignal" because it isn't an instance produced by
 * Node's own internal AbortSignal (a brand check `instanceof` doesn't
 * surface — confirmed by reproducing the same failure with bare
 * `new Request(url, {signal: new AbortController().signal})`, no React Router
 * involved). By the time this file runs, jsdom's environment setup has already
 * overwritten the outer `AbortController` too, so there is no pristine native
 * reference left to restore.
 *
 * React Router only ever reads `.url` / `.method` / `.headers` / `.signal` off
 * the `Request` it builds (verified against its bundled source) — a full
 * WHATWG `Request` was never required, so a duck-typed shim that skips Node's
 * validation is a complete, lower-risk fix.
 */
class TestRequest {
    readonly url: string
    readonly method: string
    readonly headers: Headers
    readonly signal?: AbortSignal
    readonly body: unknown

    constructor(input: string | TestRequest, init: RequestInit = {}) {
        const base = typeof input === 'string' ? undefined : input
        this.url = typeof input === 'string' ? input : input.url
        this.method = (init.method ?? base?.method ?? 'GET').toUpperCase()
        this.headers = init.headers instanceof Headers ? init.headers : new Headers(init.headers ?? base?.headers)
        this.signal = init.signal ?? base?.signal
        this.body = init.body ?? base?.body ?? null
    }

    clone(): TestRequest {
        return new TestRequest(this.url, {
            method: this.method,
            headers: this.headers,
            signal: this.signal,
            body: this.body as BodyInit | null,
        })
    }
}
Object.defineProperty(window, 'Request', {configurable: true, value: TestRequest})

/** jsdom doesn't implement matchMedia; useTheme falls back to it when no theme
 * is stored yet. Reports "light" (no system dark-mode preference) always. */
if (!window.matchMedia) {
    window.matchMedia = (query: string) => ({
        matches: false,
        media: query,
        onchange: null,
        addListener: () => {},
        removeListener: () => {},
        addEventListener: () => {},
        removeEventListener: () => {},
        dispatchEvent: () => false,
    }) as unknown as MediaQueryList
}

afterEach(() => {
    cleanup()
    sessionStorage.clear()
    localStorage.clear()
    document.documentElement.classList.remove('dark')
})
