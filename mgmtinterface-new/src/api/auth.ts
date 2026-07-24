/**
 * Client-side authentication for the frozen mgmtinterface API.
 *
 * The API only accepts HTTP Basic auth (`Authorization: Basic base64(user:pass)`)
 * and ignores session cookies, so the browser must replay the credential on every
 * request. We validate credentials once via the public `/api/users/auth/`
 * endpoint, then keep the Basic token for the session.
 *
 * SECURITY (known, accepted risk — see PR discussion / GitLab #156, #172):
 * Basic auth means the stored token is base64(username:password) — reversible
 * encoding, i.e. the user's *reusable IFF/ER-C Unix password*. It lives in
 * `localStorage`, which any XSS on the page can read. This is an accepted
 * interim compromise: the proper fix (httpOnly cookie / short-lived bearer token
 * the JS never sees) requires a backend change and is owned by the FastAPI
 * rewrite (#172). To keep that future swap to a single file, all storage access
 * goes through `tokenStore` below — replace its internals (e.g. with an
 * in-memory or cookie-backed store) without touching callers.
 */

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
 * Validate credentials against the public auth endpoint and, on success, persist
 * the Basic token for the session. Does not throw on bad credentials — returns a
 * discriminated result the login screen can render.
 */
export async function login(
    username: string,
    password: string,
): Promise<LoginResult> {
    let res: Response
    try {
        res = await fetch('/api/users/auth/', {
            method: 'POST',
            headers: {'Content-Type': 'application/json'},
            body: JSON.stringify({username, password}),
        })
    } catch {
        return {ok: false, message: 'Could not reach the server. Please try again.'}
    }

    let body: { errno?: number; message?: string } = {}
    try {
        body = await res.json()
    } catch {
        /* fall through to status-based handling */
    }

    if (res.ok && body.errno === 0) {
        tokenStore.write({username, token: basicToken(username, password)})
        return {ok: true}
    }

    return {
        ok: false,
        message: body.message || 'Sign-in failed. Check your username and password.',
    }
}

export function logout(): void {
    tokenStore.clear()
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
