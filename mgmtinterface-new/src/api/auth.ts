/**
 * Client-side authentication against the new API (GitLab #172).
 *
 * Signing in happens against `POST /mgmt-api/auth/login`, which authenticates
 * and sets an **httpOnly session cookie** the browser attaches to every `/mgmt-api`
 * request on its own. JS never sees that cookie, and the password is sent once
 * and not kept.
 *
 * What this file does keep is who signed in, so the header can show the name and
 * the route guard can send a signed-out visitor to /login without a round trip.
 * That record is a hint, not a credential: the cookie decides. When the two
 * disagree (the cookie expired, or another tab signed out), the next request
 * answers 401, `apiRequestV2` calls `clearSession`, and the loaders redirect to
 * /login, which then shows its form.
 */

// `Unauthorized` extends `ApiError`, so catching the latter covers a 401 too.
import {ApiError} from './errors'
import {apiFetchV2} from './clientV2'

const SESSION_KEY = 'erc.auth'

interface Session {
    username: string
}

/**
 * The single, swappable persistence layer for the session record. Uses
 * `localStorage` so a signed-in session is shared across every tab and window of
 * the same origin (GitLab #204 — opening a file in a new tab must not force a
 * re-login), just as the cookie is. Swap this object's body to change the storage
 * strategy app-wide.
 */
const sessionStore = {
    read(): Session | null {
        try {
            const raw = localStorage.getItem(SESSION_KEY)
            if (!raw) return null
            const stored = JSON.parse(raw) as Partial<Session> & {token?: unknown}
            if (typeof stored.username !== 'string') return null
            // Until #172 this record also held base64(username:password) for the
            // legacy Flask API. Rewrite such a record without it, so the password
            // doesn't stay in the browser of everyone who never signs out.
            if ('token' in stored) sessionStore.write({username: stored.username})
            return {username: stored.username}
        } catch {
            return null
        }
    },
    write(session: Session): void {
        try {
            localStorage.setItem(SESSION_KEY, JSON.stringify(session))
        } catch {
            /* storage unavailable (private mode / disabled) */
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

export type LoginResult = { ok: true } | { ok: false; message: string }

/**
 * Sign in: the API sets the session cookie, and the name it signed in is
 * remembered for the header and the route guard.
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
    let session: {username: string; persistent: boolean}
    try {
        session = await apiFetchV2<{username: string; persistent: boolean}>('/auth/login', {
            method: 'POST',
            body: JSON.stringify({username, password, persistent}),
        })
    } catch (err) {
        if (err instanceof ApiError) {
            return {ok: false, message: err.message}
        }
        return {ok: false, message: 'Could not reach the server. Please try again.'}
    }

    sessionStore.write({username: session.username})
    return {ok: true}
}

/**
 * Sign out.
 *
 * The local record is cleared even when the server call fails: the user asked to
 * be signed out, and showing them as signed in because the network blipped is the
 * wrong failure mode. The cookie then expires on its own.
 */
export async function logout(): Promise<void> {
    try {
        await apiFetchV2<void>('/auth/logout', {method: 'POST'})
    } catch {
        /* clearing the local record matters more than the round trip */
    } finally {
        sessionStore.clear()
    }
}

export function getCurrentUser(): string | null {
    return sessionStore.read()?.username ?? null
}

export function isAuthenticated(): boolean {
    return sessionStore.read() !== null
}

/** Used by the API client to drop the session record on a 401. */
export function clearSession(): void {
    sessionStore.clear()
}
