import {afterEach, beforeEach, describe, expect, it, vi} from 'vitest'
import {getCurrentUser, isAuthenticated, login, logout} from './auth'

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

/** The JSON body of the first request made through a stubbed `fetch`. */
function sentBody(fetchFn: ReturnType<typeof vi.fn>): unknown {
    const init = fetchFn.mock.calls[0][1] as RequestInit
    return JSON.parse(init.body as string)
}

beforeEach(() => {
    localStorage.clear()
})

afterEach(() => {
    vi.unstubAllGlobals()
})

describe('login', () => {
    it('signs in against the v2 endpoint and remembers only who signed in', async () => {
        const fetchFn = mockFetchOnce({username: 'barthel', persistent: false})

        const result = await login('barthel', 's3cret')

        expect(result).toEqual({ok: true})
        expect(fetchFn).toHaveBeenCalledWith(
            '/mgmt-api/auth/login',
            expect.objectContaining({method: 'POST', credentials: 'same-origin'}),
        )
        // A plain sign-in asks for a normal session.
        expect(sentBody(fetchFn)).toEqual({
            username: 'barthel',
            password: 's3cret',
            persistent: false,
        })
        // The session cookie is httpOnly, so nothing is asserted about it here —
        // the browser holds it. The password is sent once and kept nowhere.
        expect(JSON.parse(localStorage.getItem('erc.auth')!)).toEqual({username: 'barthel'})
        expect(getCurrentUser()).toBe('barthel')
        expect(isAuthenticated()).toBe(true)
    })

    it('sends persistent: true for "keep me logged in"', async () => {
        const fetchFn = mockFetchOnce({username: 'barthel', persistent: true})

        await expect(login('barthel', 's3cret', true)).resolves.toEqual({ok: true})

        expect(sentBody(fetchFn)).toEqual({
            username: 'barthel',
            password: 's3cret',
            persistent: true,
        })
        // The kind of session lives in the httpOnly cookie; the stored
        // record is the same either way.
        expect(localStorage.getItem('erc.auth')).not.toContain('persistent')
    })

    it('does not store anything when credentials are rejected', async () => {
        mockFetchOnce(
            {code: 'invalid_credentials', message: 'Wrong username or password.'},
            {status: 401},
        )

        const result = await login('barthel', 'wrong')

        expect(result).toEqual({ok: false, message: 'Wrong username or password.'})
        expect(localStorage.getItem('erc.auth')).toBeNull()
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
})

describe('the remembered sign-in', () => {
    it('drops the password from a record stored before #172', () => {
        localStorage.setItem(
            'erc.auth',
            JSON.stringify({username: 'barthel', token: btoa('barthel:s3cret')}),
        )

        expect(getCurrentUser()).toBe('barthel')
        // The user stays signed in; the reversible password is gone from storage.
        expect(JSON.parse(localStorage.getItem('erc.auth')!)).toEqual({username: 'barthel'})
    })

    it('treats a record without a username as signed out', () => {
        localStorage.setItem('erc.auth', JSON.stringify({token: 'dXNlcjpwYXNz'}))

        expect(isAuthenticated()).toBe(false)
    })
})

describe('logout', () => {
    it('ends the server session and clears the remembered sign-in', async () => {
        mockFetchOnce({username: 'barthel'})
        await login('barthel', 's3cret')
        expect(isAuthenticated()).toBe(true)

        const fetchFn = mockFetchOnce(null, {status: 204})
        await logout()

        expect(fetchFn).toHaveBeenCalledWith(
            '/mgmt-api/auth/logout',
            expect.objectContaining({method: 'POST'}),
        )
        expect(isAuthenticated()).toBe(false)
        expect(getCurrentUser()).toBeNull()
    })

    it('still clears the remembered sign-in when the server cannot be reached', async () => {
        mockFetchOnce({username: 'barthel'})
        await login('barthel', 's3cret')

        vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new Error('network down')))
        await logout()

        // Showing the user as signed in because the network blipped would be
        // the wrong way to fail a sign-out.
        expect(isAuthenticated()).toBe(false)
    })
})
