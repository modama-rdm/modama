import {afterEach, describe, expect, it, vi} from 'vitest'
import {ApiError, Unauthorized} from './client'
import {apiFetchV2, parseError} from './clientV2'

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
            '/api/v2/auth/session',
            expect.objectContaining({credentials: 'same-origin'}),
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
})
