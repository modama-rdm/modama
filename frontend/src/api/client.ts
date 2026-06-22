/**
 * Central fetch wrapper for the mgmtinterface `/api/*` endpoints.
 *
 * Injects the HTTP Basic credential on every call and centralises error
 * handling. A 401 means the stored credential is missing/expired/invalid, so we
 * drop the session and throw `Unauthorized` — router loaders catch this and
 * redirect to /login (preserving the attempted URL via ?next).
 */

import { clearSession, getToken } from './auth'

export class ApiError extends Error {
  readonly status: number
  constructor(status: number, message: string) {
    super(message)
    this.name = 'ApiError'
    this.status = status
  }
}

export class Unauthorized extends ApiError {
  constructor(message = 'Unauthorized') {
    super(401, message)
    this.name = 'Unauthorized'
  }
}

/**
 * Typed request to an `/api/*` path. Returns the parsed JSON body. Throws
 * `Unauthorized` on 401 (after clearing the session) and `ApiError` on other
 * non-2xx responses. Callers still check the `errno` field for logical errors
 * the API reports with a 200.
 */
export async function apiFetch<T>(path: string, init: RequestInit = {}): Promise<T> {
  const token = getToken()
  const headers = new Headers(init.headers)
  if (token) headers.set('Authorization', `Basic ${token}`)
  if (init.body && !headers.has('Content-Type')) {
    headers.set('Content-Type', 'application/json')
  }

  const res = await fetch(path, { ...init, headers })

  if (res.status === 401) {
    clearSession()
    throw new Unauthorized()
  }

  if (!res.ok) {
    const message = await res.text().catch(() => '')
    throw new ApiError(res.status, message || `Request failed (${res.status})`)
  }

  // 204 No Content / empty body
  if (res.status === 204) return undefined as T
  return (await res.json()) as T
}
