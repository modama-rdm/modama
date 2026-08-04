/**
 * Fetch wrapper for the new FastAPI endpoints under `/api/v2` (GitLab #172).
 *
 * Deliberately separate from `apiFetch`, which speaks the legacy Flask dialect:
 * that API answers HTTP 200 with an `{errno}` envelope and authenticates with a
 * Basic header, while this one uses real status codes and an httpOnly session
 * cookie the browser attaches on its own. One wrapper trying to speak both would
 * be harder to read than two that each speak one — and this file is the only
 * place that knows the wire format, so changing it later is a single edit.
 *
 * The two coexist until every screen has moved over; see `auth.ts`.
 */

import {ApiError, Unauthorized} from './client'

export const API_V2 = '/api/v2'

/** Error body this API promises: a stable `code` plus a displayable `message`. */
interface ErrorBody {
    code: string
    message: string
}

/** FastAPI's own validation shape — the documented exception to the above. */
interface ValidationBody {
    detail: {loc: (string | number)[]; msg: string; type: string}[]
}

function isErrorBody(body: unknown): body is ErrorBody {
    if (typeof body !== 'object' || body === null) return false
    const candidate = body as Partial<ErrorBody>
    return typeof candidate.code === 'string' && typeof candidate.message === 'string'
}

function isValidationBody(body: unknown): body is ValidationBody {
    if (typeof body !== 'object' || body === null) return false
    return Array.isArray((body as Partial<ValidationBody>).detail)
}

/**
 * Normalize any non-2xx body into `{code, message}`.
 *
 * Screens read the result of this, never the raw body, so the wire format stays
 * changeable in one place. Handles three cases: the house model, FastAPI's 422,
 * and anything else (a proxy error page, a truncated response).
 */
export function parseError(status: number, body: unknown): ErrorBody {
    if (isErrorBody(body)) return body

    if (isValidationBody(body)) {
        const first = body.detail[0]
        return {
            code: 'invalid_request',
            message: first?.msg ?? 'Some of the values you entered are not valid.',
        }
    }

    return {
        code: 'unexpected_error',
        message: `Request failed (${status}).`,
    }
}

/**
 * Request to an `/api/v2` path, returning the parsed JSON body.
 *
 * Throws `Unauthorized` on 401 so existing loaders keep redirecting to /login
 * unchanged, and `ApiError` carrying the API's own message on any other non-2xx
 * — that message is written to be shown to the user.
 */
export async function apiFetchV2<T>(path: string, init: RequestInit = {}): Promise<T> {
    const headers = new Headers(init.headers)
    if (init.body && !headers.has('Content-Type')) {
        headers.set('Content-Type', 'application/json')
    }

    const res = await fetch(`${API_V2}${path}`, {
        ...init,
        headers,
        // The session cookie is httpOnly, so JS never sees it; the browser has
        // to be told to send it. Same-origin behind the proxy, so no CORS.
        credentials: 'same-origin',
    })

    if (res.ok) {
        if (res.status === 204) return undefined as T
        return (await res.json()) as T
    }

    const body: unknown = await res.json().catch(() => null)
    const error = parseError(res.status, body)

    if (res.status === 401) {
        throw new Unauthorized(error.message)
    }
    throw new ApiError(res.status, error.message)
}
