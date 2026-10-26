/**
 * Fetch wrapper for the FastAPI endpoints under `/mgmt-api` (GitLab #172).
 *
 * The API uses real status codes and an httpOnly session cookie the browser
 * attaches on its own. This file is the only place that knows the wire format,
 * so changing it later is a single edit.
 */

import {clearSession} from './auth'
import {ApiError, Unauthorized} from './errors'

export const MGMT_API = '/mgmt-api'

/**
 * The API contract this build was written against, sent on every request.
 *
 * The server refuses a request that does not state one rather than guessing —
 * it serves several contracts at once for the sake of clients (modama-agent and
 * friends) that are installed on lab machines and updated slowly. This app is
 * served by the same deployment as the API, so it can simply move with it: bump
 * this constant in the same commit that starts using a newer contract.
 */
export const API_VERSION = 1

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

/** A successful answer: the parsed body and the status it came with. */
export interface ApiResult<T> {
    status: number
    body: T
}

/**
 * Request to a `/mgmt-api` path, returning the parsed JSON body together with
 * the status code.
 *
 * The status matters for the create endpoints, which are idempotent: 201 means
 * the thing was just made, 200 that it already existed and was returned as is.
 * Most callers do not care and use `apiFetchV2`.
 *
 * Throws `Unauthorized` on 401, so loaders redirect to /login, and `ApiError`
 * carrying the API's own message on any other non-2xx — that message is written
 * to be shown to the user. A 401 also clears the remembered sign-in: the login
 * screen would otherwise take it as current and send the user straight back.
 */
export async function apiRequestV2<T>(
    path: string,
    init: RequestInit = {},
): Promise<ApiResult<T>> {
    const headers = new Headers(init.headers)
    if (init.body && !headers.has('Content-Type')) {
        headers.set('Content-Type', 'application/json')
    }
    headers.set('X-Modama-API-Version', String(API_VERSION))

    const res = await fetch(`${MGMT_API}${path}`, {
        ...init,
        headers,
        // The session cookie is httpOnly, so JS never sees it; the browser has
        // to be told to send it. Same-origin behind the proxy, so no CORS.
        credentials: 'same-origin',
    })

    if (res.ok) {
        if (res.status === 204) return {status: res.status, body: undefined as T}
        return {status: res.status, body: (await res.json()) as T}
    }

    const body: unknown = await res.json().catch(() => null)
    const error = parseError(res.status, body)

    if (res.status === 401) {
        clearSession()
        throw new Unauthorized(error.message)
    }
    throw new ApiError(res.status, error.message)
}

/** `apiRequestV2` for callers that only need the body. */
export async function apiFetchV2<T>(path: string, init: RequestInit = {}): Promise<T> {
    return (await apiRequestV2<T>(path, init)).body
}
