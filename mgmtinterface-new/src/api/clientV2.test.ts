import {afterEach, describe, expect, it, vi} from 'vitest'
import {isAuthenticated} from './auth'
import {ApiError, Unauthorized} from './errors'
import {API_VERSION, apiFetchV2, apiRequestV2, parseError} from './clientV2'

function mockFetchOnce(
    body: unknown,
    init: {ok?: boolean; status?: number} = {},
) {
    const status = init.status ?? 200
    const res = {
        ok: init.ok ?? status < 400,
        status,
        json: async () => body,
    } as Response
    const fn = vi.fn().mockResolvedValue(res)
    vi.stubGlobal('fetch', fn)
    return fn
}

afterEach(() => {
    vi.unstubAllGlobals()
    localStorage.clear()
})

describe('parseError', () => {
    it('passes the house model through', () => {
        expect(parseError(403, {code: 'not_a_member', message: 'Nope.'})).toEqual({
            code: 'not_a_member',
            message: 'Nope.',
        })
    })

    it("reads FastAPI's 422 shape, which does not use the house model", () => {
        const body = {
            detail: [{loc: ['body', 'username'], msg: 'Field required', type: 'missing'}],
        }
        expect(parseError(422, body)).toEqual({
            code: 'invalid_request',
            message: 'Field required',
        })
    })

    it('falls back for bodies from outside the API (proxy pages, truncation)', () => {
        expect(parseError(502, '<html>Bad Gateway</html>')).toEqual({
            code: 'unexpected_error',
            message: 'Request failed (502).',
        })
        expect(parseError(500, null)).toEqual({
            code: 'unexpected_error',
            message: 'Request failed (500).',
        })
    })
})

describe('apiFetchV2', () => {
    it('prefixes the path and sends the session cookie', async () => {
        const fetchFn = mockFetchOnce({username: 'barthel'})

        await apiFetchV2('/auth/session')

        expect(fetchFn).toHaveBeenCalledWith(
            '/mgmt-api/auth/session',
            expect.objectContaining({credentials: 'same-origin'}),
        )
    })

    it('states the API version this build was written against', async () => {
        // The server refuses a request that does not, rather than guessing a
        // version for it — see the FastAPI service's versioning.py.
        const fetchFn = mockFetchOnce({username: 'barthel'})

        await apiFetchV2('/auth/session')

        const [, init] = fetchFn.mock.calls[0] as [string, RequestInit]
        expect(new Headers(init.headers).get('X-Modama-API-Version')).toBe(
            String(API_VERSION),
        )
    })

    it('sets a JSON content type only when there is a body', async () => {
        const fetchFn = mockFetchOnce({username: 'barthel'})
        await apiFetchV2('/auth/login', {
            method: 'POST',
            body: JSON.stringify({username: 'barthel'}),
        })
        const withBody = fetchFn.mock.calls[0][1] as RequestInit
        expect((withBody.headers as Headers).get('Content-Type')).toBe('application/json')

        const bare = mockFetchOnce({username: 'barthel'})
        await apiFetchV2('/auth/session')
        const withoutBody = bare.mock.calls[0][1] as RequestInit
        expect((withoutBody.headers as Headers).get('Content-Type')).toBeNull()
    })

    it('reports the status alongside the body through apiRequestV2', async () => {
        // The create endpoints are idempotent and answer 201 for a new thing
        // and 200 for one that already existed; screens word their toast on it.
        mockFetchOnce({name: 'ghost_imaging_2026'}, {status: 201})
        await expect(
            apiRequestV2('/projects', {method: 'POST', body: '{}'}),
        ).resolves.toEqual({status: 201, body: {name: 'ghost_imaging_2026'}})

        mockFetchOnce({name: 'ghost_imaging_2026'}, {status: 200})
        await expect(
            apiRequestV2('/projects', {method: 'POST', body: '{}'}),
        ).resolves.toEqual({status: 200, body: {name: 'ghost_imaging_2026'}})
    })

    it('returns undefined for 204 rather than parsing an empty body', async () => {
        mockFetchOnce(null, {status: 204})
        await expect(apiFetchV2('/auth/logout', {method: 'POST'})).resolves.toBeUndefined()
    })

    it('throws Unauthorized on 401 so loaders redirect to /login', async () => {
        mockFetchOnce({code: 'session_expired', message: 'Your session has expired.'}, {
            status: 401,
        })
        await expect(apiFetchV2('/auth/session')).rejects.toBeInstanceOf(Unauthorized)
    })

    it('forgets the remembered sign-in on 401', async () => {
        localStorage.setItem('erc.auth', JSON.stringify({username: 'barthel'}))
        mockFetchOnce({code: 'session_expired', message: 'Your session has expired.'}, {
            status: 401,
        })

        await expect(apiFetchV2('/projects')).rejects.toBeInstanceOf(Unauthorized)
        // Otherwise /login takes the stale record as current and sends the user
        // straight back to the page that just failed.
        expect(isAuthenticated()).toBe(false)
    })

    it('keeps the sign-in on other failures', async () => {
        localStorage.setItem('erc.auth', JSON.stringify({username: 'barthel'}))
        mockFetchOnce({code: 'permissions_unavailable', message: 'Unavailable.'}, {
            status: 502,
        })

        await expect(apiFetchV2('/projects')).rejects.toBeInstanceOf(ApiError)
        // A server error must not sign the user out.
        expect(isAuthenticated()).toBe(true)
    })

    it("carries the API's own message on other failures", async () => {
        mockFetchOnce(
            {code: 'too_many_attempts', message: 'Too many failed sign-in attempts.'},
            {status: 429},
        )
        await expect(apiFetchV2('/auth/login', {method: 'POST'})).rejects.toThrow(
            'Too many failed sign-in attempts.',
        )
        await expect(apiFetchV2('/auth/login', {method: 'POST'})).rejects.toBeInstanceOf(
            ApiError,
        )
    })

    it("carries the API's code, for screens that branch on it", async () => {
        mockFetchOnce(
            {code: 'description_not_saved', message: 'The project was created, but…'},
            {status: 502},
        )
        await expect(apiFetchV2('/projects', {method: 'POST'})).rejects.toMatchObject({
            status: 502,
            code: 'description_not_saved',
        })
    })
})
