/**
 * Mount the real application — the same route tree, loaders and providers that
 * `main.tsx` wires up — under a memory router at an arbitrary entry.
 *
 * Journeys drive this exactly as a user would: they start at a URL, click, type,
 * and assert on what is rendered. Nothing here knows about fixtures; the data
 * comes from whatever `installApiMock` was given (see `api-mock.ts`).
 */

import {render} from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import {RouterProvider, createMemoryRouter} from 'react-router-dom'
import {ToastProvider} from '../components/ui/Toast'
import {routes} from '../routeTree'

export interface AppOptions {
    /** Initial URL, app-relative (the memory router carries no basename). */
    route?: string
}

export function renderApp({route = '/'}: AppOptions = {}) {
    const router = createMemoryRouter(routes, {initialEntries: [route]})
    const user = userEvent.setup()
    const utils = render(
        <ToastProvider>
            <RouterProvider router={router}/>
        </ToastProvider>,
    )
    return {...utils, user, router}
}

/**
 * Put an authenticated session in place, mirroring what `login()` persists, so a
 * journey that isn't testing login can start behind the auth gate. The shape is
 * `api/auth.ts`'s `Session` — if that changes, this fails loudly and correctly.
 */
export function signIn(username = 'user1', password = 'correct-horse'): void {
    const token = btoa(`${username}:${password}`)
    sessionStorage.setItem('erc.auth', JSON.stringify({username, token}))
}
