import {afterEach, beforeEach, describe, expect, it, vi} from 'vitest'
import {
    getCurrentUser,
    getToken,
    isAuthenticated,
    login,
    logout,
} from './auth'

function mockFetchOnce(body: unknown, init: { ok?: boolean; status?: number } = {}) {
    const res = {
        ok: init.ok ?? true,
        status: init.status ?? 200,
        json: async () => body,
    } as Response
    const fn = vi.fn().mockResolvedValue(res)
    vi.stubGlobal('fetch', fn)
    return fn
}

beforeEach(() => {
    sessionStorage.clear()
})

afterEach(() => {
    vi.unstubAllGlobals()
})

describe('login', () => {
    it('stores the Basic token on success and reports the user', async () => {
        const fetchFn = mockFetchOnce({errno: 0, id: {uid: 1234}})

        const result = await login('barthel', 's3cret')

        expect(result).toEqual({ok: true})
        // Posts JSON credentials to the public auth endpoint.
        expect(fetchFn).toHaveBeenCalledWith(
            '/api/users/auth/',
            expect.objectContaining({method: 'POST'}),
        )
        // Token is base64(username:password) — the Authorization: Basic value.
        expect(getToken()).toBe(btoa('barthel:s3cret'))
        expect(getCurrentUser()).toBe('barthel')
        expect(isAuthenticated()).toBe(true)
    })

    it('does not store anything when credentials are rejected', async () => {
        mockFetchOnce({errno: 1, message: 'bad password'})

        const result = await login('barthel', 'wrong')

        expect(result).toEqual({ok: false, message: 'bad password'})
        expect(getToken()).toBeNull()
        expect(isAuthenticated()).toBe(false)
    })

    it('returns a friendly error when the request fails', async () => {
        vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new Error('network down')))

        const result = await login('barthel', 's3cret')

        expect(result.ok).toBe(false)
        expect(isAuthenticated()).toBe(false)
    })

    it('encodes non-ASCII passwords as UTF-8 (btoa would otherwise throw)', async () => {
        mockFetchOnce({errno: 0, id: {uid: 1}})

        await expect(login('müller', 'pä$$wörd')).resolves.toEqual({ok: true})
        expect(getToken()).toBeTypeOf('string')
    })
})

describe('logout', () => {
    it('clears the stored session', async () => {
        mockFetchOnce({errno: 0, id: {uid: 1}})
        await login('barthel', 's3cret')
        expect(isAuthenticated()).toBe(true)

        logout()

        expect(isAuthenticated()).toBe(false)
        expect(getCurrentUser()).toBeNull()
        expect(getToken()).toBeNull()
    })
})
