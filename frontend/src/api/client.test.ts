import {afterEach, beforeEach, describe, expect, it, vi} from 'vitest'
import {ApiError, Unauthorized, apiFetch} from './client'
import {isAuthenticated, login} from './auth'

function mockResponse(body: unknown, init: { ok?: boolean; status?: number } = {}) {
    return {
        ok: init.ok ?? true,
        status: init.status ?? 200,
        json: async () => body,
        text: async () => (typeof body === 'string' ? body : JSON.stringify(body)),
    } as Response
}

async function seedSession() {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(mockResponse({errno: 0, id: {uid: 1}})))
    await login('ritwik', 's3cret')
}

beforeEach(() => {
    sessionStorage.clear()
})

afterEach(() => {
    vi.unstubAllGlobals()
})

describe('apiFetch', () => {
    it('injects the Authorization: Basic header from the stored session', async () => {
        await seedSession()
        const fetchFn = vi.fn().mockResolvedValue(mockResponse({errno: 0, projects: []}))
        vi.stubGlobal('fetch', fetchFn)

        await apiFetch('/api/project/')

        const headers = (fetchFn.mock.calls[0][1] as RequestInit).headers as Headers
        expect(headers.get('Authorization')).toBe(`Basic ${btoa('ritwik:s3cret')}`)
    })

    it('returns the parsed JSON body on success', async () => {
        await seedSession()
        const payload = {errno: 0, projects: [{name: 'ghost_imaging'}]}
        vi.stubGlobal('fetch', vi.fn().mockResolvedValue(mockResponse(payload)))

        const data = await apiFetch<typeof payload>('/api/project/')

        expect(data).toEqual(payload)
    })

    it('clears the session and throws Unauthorized on 401', async () => {
        await seedSession()
        expect(isAuthenticated()).toBe(true)
        vi.stubGlobal('fetch', vi.fn().mockResolvedValue(mockResponse('nope', {ok: false, status: 401})))

        await expect(apiFetch('/api/project/')).rejects.toBeInstanceOf(Unauthorized)
        // Session is dropped so loaders redirect to /login.
        expect(isAuthenticated()).toBe(false)
    })

    it('throws ApiError (not Unauthorized) on other non-2xx responses', async () => {
        await seedSession()
        vi.stubGlobal('fetch', vi.fn().mockResolvedValue(mockResponse('boom', {ok: false, status: 500})))

        await expect(apiFetch('/api/project/')).rejects.toBeInstanceOf(ApiError)
        // A server error must not log the user out.
        expect(isAuthenticated()).toBe(true)
    })

    it('omits the Authorization header when there is no session', async () => {
        const fetchFn = vi.fn().mockResolvedValue(mockResponse({errno: 0}))
        vi.stubGlobal('fetch', fetchFn)

        await apiFetch('/api/project/')

        const headers = (fetchFn.mock.calls[0][1] as RequestInit).headers as Headers
        expect(headers.has('Authorization')).toBe(false)
    })
})
