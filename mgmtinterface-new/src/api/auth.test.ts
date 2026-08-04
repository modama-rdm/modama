import {afterEach, beforeEach, describe, expect, it, vi} from 'vitest'
import {
    getCurrentUser,
    getToken,
    isAuthenticated,
    login,
    logout,
} from './auth'

function mockFetchOnce(body: unknown, init: { ok?: boolean; status?: number } = {}) {
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

beforeEach(() => {
    localStorage.clear()
})

afterEach(() => {
    vi.unstubAllGlobals()
})

describe('login', () => {
    it('signs in against the v2 endpoint and keeps the Basic token for legacy screens', async () => {
        const fetchFn = mockFetchOnce({username: 'barthel'})

        const result = await login('barthel', 's3cret')

        expect(result).toEqual({ok: true})
        expect(fetchFn).toHaveBeenCalledWith(
            '/api/v2/auth/login',
            expect.objectContaining({method: 'POST', credentials: 'same-origin'}),
        )
        // The session cookie is httpOnly, so nothing is asserted about it here —
        // the browser holds it. The token below exists only because the other
        // screens still call the legacy Flask API.
        expect(getToken()).toBe(btoa('barthel:s3cret'))
        expect(getCurrentUser()).toBe('barthel')
        expect(isAuthenticated()).toBe(true)
    })

    it('does not store anything when credentials are rejected', async () => {
        mockFetchOnce(
            {code: 'invalid_credentials', message: 'Wrong username or password.'},
            {status: 401},
        )

        const result = await login('barthel', 'wrong')

        expect(result).toEqual({ok: false, message: 'Wrong username or password.'})
        expect(getToken()).toBeNull()
        expect(isAuthenticated()).toBe(false)
    })

    it("surfaces the API's message when sign-in is throttled", async () => {
        mockFetchOnce(
            {
                code: 'too_many_attempts',
                message: 'Too many failed sign-in attempts. Please wait and try again.',
            },
            {status: 429},
        )

        const result = await login('barthel', 'wrong')

        expect(result).toEqual({
            ok: false,
            message: 'Too many failed sign-in attempts. Please wait and try again.',
        })
        expect(isAuthenticated()).toBe(false)
    })

    it('distinguishes an unreachable account service from bad credentials', async () => {
        mockFetchOnce(
            {
                code: 'permissions_unavailable',
                message: 'The account service is unavailable. Please try again shortly.',
            },
            {status: 502},
        )

        const result = await login('barthel', 's3cret')

        expect(result).toEqual({
            ok: false,
            message: 'The account service is unavailable. Please try again shortly.',
        })
    })

    it('returns a friendly error when the request fails', async () => {
        vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new Error('network down')))

        const result = await login('barthel', 's3cret')

        expect(result).toEqual({
            ok: false,
            message: 'Could not reach the server. Please try again.',
        })
        expect(isAuthenticated()).toBe(false)
    })

    it('encodes non-ASCII passwords as UTF-8 (btoa would otherwise throw)', async () => {
        mockFetchOnce({username: 'müller'})

        await expect(login('müller', 'pä$$wörd')).resolves.toEqual({ok: true})
        expect(getToken()).toBeTypeOf('string')
    })
})

describe('logout', () => {
    it('ends the server session and clears the stored credential', async () => {
        mockFetchOnce({username: 'barthel'})
        await login('barthel', 's3cret')
        expect(isAuthenticated()).toBe(true)

        const fetchFn = mockFetchOnce(null, {status: 204})
        await logout()

        expect(fetchFn).toHaveBeenCalledWith(
            '/api/v2/auth/logout',
            expect.objectContaining({method: 'POST'}),
        )
        expect(isAuthenticated()).toBe(false)
        expect(getCurrentUser()).toBeNull()
        expect(getToken()).toBeNull()
    })

    it('still clears the credential when the server cannot be reached', async () => {
        mockFetchOnce({username: 'barthel'})
        await login('barthel', 's3cret')

        vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new Error('network down')))
        await logout()

        // Leaving a usable credential behind because the network blipped would
        // be the wrong way to fail a sign-out.
        expect(isAuthenticated()).toBe(false)
    })
})
