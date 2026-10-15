/**
 * Client-side authentication, mid-migration between two APIs (GitLab #172).
 *
 * Signing in now happens against `POST /mgmt-api/auth/login`, which authenticates
 * and sets an **httpOnly session cookie** the browser attaches to every `/mgmt-api`
 * request on its own. JS never sees that cookie.
 *
 * The Basic token below has *not* gone away yet, because most screens still call
 * the legacy Flask `/api/*` endpoints, and those accept nothing but
 * `Authorization: Basic`. So one sign-in establishes both credentials, and the
 * token is retired when the last screen moves to `/mgmt-api` — at which point
 * `tokenStore`, `getToken`, and the backend's `xBasic` handler all go together.
 *
 * SECURITY (known, accepted, and shrinking — GitLab #156, #172): the Basic token
 * is base64(username:password), i.e. reversible encoding of the user's reusable
 * IFF/ER-C Unix password, held in `localStorage` where any XSS can read it. The
 * cookie has no such exposure. Every endpoint migrated to `/mgmt-api` is one less
 * reason for the token to exist.
 */

// `Unauthorized` extends `ApiError`, so catching the latter covers a 401 too.
import {ApiError} from './client'
import {apiFetchV2} from './clientV2'

const SESSION_KEY = 'erc.auth'

interface Session {
    username: string
    /** base64(username:password) for the Authorization: Basic header. */
    token: string
}

/**
 * The single, swappable persistence layer for the credential. Uses
 * `localStorage` so a signed-in session is shared across every tab and window of
 * the same origin (GitLab #204 — opening a file in a new tab must not force a
 * re-login). This trades the per-tab isolation of sessionStorage for that shared
 * session; the XSS exposure is unchanged (either store is readable by page JS).
 * Swap this object's body to change the storage strategy app-wide.
 */
const tokenStore = {
    read(): Session | null {
        try {
            const raw = localStorage.getItem(SESSION_KEY)
            return raw ? (JSON.parse(raw) as Session) : null
        } catch {
            return null
        }
    },
    write(session: Session): void {
        try {
            localStorage.setItem(SESSION_KEY, JSON.stringify(session))
        } catch {
            /* storage unavailable (private mode / disabled) — auth degrades to in-tab only */
        }
    },
    clear(): void {
        try {
            localStorage.removeItem(SESSION_KEY)
        } catch {
            /* ignore */
        }
    },
}

/** UTF-8 safe base64 — plain btoa throws on non-Latin1 characters. */
function basicToken(username: string, password: string): string {
    const bytes = new TextEncoder().encode(`${username}:${password}`)
    let binary = ''
    for (const b of bytes) binary += String.fromCharCode(b)
    return btoa(binary)
}

export type LoginResult = { ok: true } | { ok: false; message: string }

/**
 * Sign in: establishes the session cookie and, for as long as legacy screens
 * need it, the Basic token.
 *
 * `persistent` is the "keep me logged in" choice: the server then issues a
 * session without an idle timeout and a longer absolute lifetime. The kind of
 * session lives in the httpOnly cookie, so nothing about it is stored here.
 *
 * Does not throw on bad credentials — returns a discriminated result the login
 * screen renders. The messages come from the API itself, so a throttled sign-in
 * (429) explains itself without this file knowing about throttling.
 */
export async function login(
    username: string,
    password: string,
    persistent = false,
): Promise<LoginResult> {
    try {
        await apiFetchV2<{username: string; persistent: boolean}>('/auth/login', {
            method: 'POST',
            body: JSON.stringify({username, password, persistent}),
        })
    } catch (err) {
        if (err instanceof ApiError) {
            return {ok: false, message: err.message}
        }
        return {ok: false, message: 'Could not reach the server. Please try again.'}
    }

    tokenStore.write({username, token: basicToken(username, password)})
    return {ok: true}
}

/**
 * Sign out of both credentials.
 *
 * The local token is cleared even when the server call fails: the user asked to
 * be signed out, and leaving a usable credential behind because the network
 * blipped is the wrong failure mode. The cookie then expires on its own.
 */
export async function logout(): Promise<void> {
    try {
        await apiFetchV2<void>('/auth/logout', {method: 'POST'})
    } catch {
        /* clearing the local credential matters more than the round trip */
    } finally {
        tokenStore.clear()
    }
}

export function getToken(): string | null {
    return tokenStore.read()?.token ?? null
}

export function getCurrentUser(): string | null {
    return tokenStore.read()?.username ?? null
}

export function isAuthenticated(): boolean {
    return tokenStore.read() !== null
}

/** Used by the API client to drop the session on a 401. */
export function clearSession(): void {
    tokenStore.clear()
}
