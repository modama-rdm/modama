import {screen, waitFor, within} from '@testing-library/react'
import {describe, expect, it} from 'vitest'
import {installApiMock, type ApiHandler} from '../api-mock'
import {renderApp, signIn} from '../render'
import {baseWorld, browseReply} from '../world'
import {FIXTURE_BROWSE} from '../fixtures/browse'

/**
 * #169 journey 6/6 — browse: a real production directory listing, chosen for
 * the exact edge cases #202 (space-containing folder names, `quotePath.ts`)
 * and MR!61 (the colored "Open in" badges) exist for. Since #90 the listing
 * comes from `GET /mgmt-api/browse` in pages; the journeys below page a
 * fixture that would fit on one page by making the mock answer four at a time.
 */
describe('journey: browse', () => {
    const route = `/browse?path=${encodeURIComponent(FIXTURE_BROWSE.path)}`

    /** `baseWorld()` with the browse handler answering `pageSize` entries per page. */
    function pagedWorld(pageSize: number, failWhen?: () => boolean): ApiHandler[] {
        return [
            {
                method: 'GET',
                path: '/mgmt-api/browse',
                respond: ({params}) =>
                    failWhen?.()
                        ? {
                              status: 502,
                              body: {
                                  code: 'files_unavailable',
                                  message: 'The file server is unavailable. Please try again shortly.',
                              },
                          }
                        : browseReply(params, pageSize),
            },
            ...baseWorld(),
        ]
    }

    it('renders the listing, breadcrumb, and access badges for a real directory', async () => {
        signIn('user1', 'correct-horse')
        const api = installApiMock(baseWorld())
        renderApp({route})

        // A folder name with a space — the exact shape #202 exists to round-trip.
        const spacedEntry = FIXTURE_BROWSE.entries.find(e => e.name.includes(' '))!
        expect(await screen.findByText(spacedEntry.name)).toBeInTheDocument()

        // Current folder's name — shown in the breadcrumb tail, the resource
        // bar title, and (since this folder is itself a project) the "in …" link.
        expect(screen.getAllByText(FIXTURE_BROWSE.name).length).toBeGreaterThanOrEqual(2)
        expect(screen.getByText(new RegExp(`${FIXTURE_BROWSE.total} items`))).toBeInTheDocument()

        // The `..` row and the breadcrumb are built from the path, not sent by the API.
        expect(screen.getByRole('link', {name: '..'})).toHaveAttribute('href', '/browse?path=/hdtms')
        expect(screen.getByRole('link', {name: 'er-c-data'})).toHaveAttribute('href', '/browse?path=/')

        // MR!61's colored "Open in" badges — every access link this fixture carries.
        expect(screen.getAllByRole('link', {name: 'Nextcloud'}).length).toBeGreaterThan(0)
        expect(screen.getAllByRole('link', {name: 'SampleDB (ER-C-3)'}).length).toBeGreaterThan(0)
        expect(screen.getAllByRole('link', {name: 'ElabFTW (ER-C-3)'}).length).toBeGreaterThan(0)

        // One page of 200 from the new API, and nothing from the legacy one (#172).
        const calls = api.matching('GET', '/mgmt-api/browse')
        expect(calls).toHaveLength(1)
        expect(calls[0].params.get('limit')).toBe('200')
        expect(api.calls.filter(c => c.path.startsWith('/api/'))).toHaveLength(0)
        // Everything fit on one page: no paging controls.
        expect(screen.queryByRole('button', {name: /load more/i})).not.toBeInTheDocument()
    })

    it('redirects to the normalized path the API answers with', async () => {
        signIn('user1', 'correct-horse')
        installApiMock(baseWorld())
        const {router} = renderApp({route: `${route}%2F`})

        expect(await screen.findByText(new RegExp(`${FIXTURE_BROWSE.total} items`))).toBeInTheDocument()
        // The address bar carries the one spelling, so the copy buttons agree with it.
        expect(router.state.location.search).toBe(`?path=${FIXTURE_BROWSE.path}`)
    })

    it('loads the next page on "Load more", keeping the rows already shown', async () => {
        signIn('user1', 'correct-horse')
        const api = installApiMock(pagedWorld(4))
        const {user} = renderApp({route})

        const firstPage = FIXTURE_BROWSE.entries.slice(0, 4)
        const lastEntry = FIXTURE_BROWSE.entries[FIXTURE_BROWSE.entries.length - 1]
        expect(await screen.findByText(firstPage[0].name)).toBeInTheDocument()
        expect(screen.getByText(`4 of ${FIXTURE_BROWSE.total}`)).toBeInTheDocument()
        expect(screen.queryByText(lastEntry.name)).not.toBeInTheDocument()

        await user.click(screen.getByRole('button', {name: /load more/i}))
        expect(await screen.findByText(`8 of ${FIXTURE_BROWSE.total}`)).toBeInTheDocument()
        // Page one is still there; the cursor of page one was sent back.
        expect(screen.getByText(firstPage[0].name)).toBeInTheDocument()
        const calls = api.matching('GET', '/mgmt-api/browse')
        expect(calls).toHaveLength(2)
        expect(calls[1].params.get('cursor')).toBeTruthy()

        await user.click(screen.getByRole('button', {name: /load more/i}))
        expect(await screen.findByText(lastEntry.name)).toBeInTheDocument()
        expect(screen.getByText(`${FIXTURE_BROWSE.total} of ${FIXTURE_BROWSE.total}`)).toBeInTheDocument()
        // Last page: the button is gone.
        expect(screen.queryByRole('button', {name: /load more/i})).not.toBeInTheDocument()
    })

    it('shows a failed "Load more" inline and retries without dropping the rows', async () => {
        signIn('user1', 'correct-horse')
        let fail = false
        installApiMock(pagedWorld(4, () => fail))
        const {user} = renderApp({route})
        const firstPage = FIXTURE_BROWSE.entries.slice(0, 4)
        expect(await screen.findByText(firstPage[0].name)).toBeInTheDocument()

        fail = true
        await user.click(screen.getByRole('button', {name: /load more/i}))
        expect(await screen.findByRole('alert')).toHaveTextContent('The file server is unavailable')
        expect(screen.getByText(firstPage[3].name)).toBeInTheDocument()
        expect(screen.getByText(`4 of ${FIXTURE_BROWSE.total}`)).toBeInTheDocument()

        fail = false
        await user.click(screen.getByRole('button', {name: /try again/i}))
        expect(await screen.findByText(`8 of ${FIXTURE_BROWSE.total}`)).toBeInTheDocument()
        await waitFor(() => expect(screen.queryByRole('alert')).not.toBeInTheDocument())
    })

    it('shows the not-found state with the breadcrumb as the way back for a bad path', async () => {
        signIn('user1', 'correct-horse')
        installApiMock(baseWorld())
        renderApp({route: '/browse?path=%2Fhdtms%2Fnope'})

        expect(await screen.findByText('Could not open this path')).toBeInTheDocument()
        expect(screen.getByText("There is no file or folder at '/hdtms/nope'.")).toBeInTheDocument()
        // Every ancestor is one click away; the missing tail is not a link.
        const nav = screen.getByRole('navigation')
        expect(within(nav).getByRole('link', {name: 'hdtms'})).toHaveAttribute('href', '/browse?path=/hdtms')
        expect(within(nav).getByText('nope')).toBeInTheDocument()
        expect(within(nav).queryByRole('link', {name: 'nope'})).not.toBeInTheDocument()
    })

    it('shows the no-access state for a folder the user may not read', async () => {
        signIn('user1', 'correct-horse')
        // Listed before `...baseWorld()`: the mock's first-match-wins.
        installApiMock([
            {
                method: 'GET',
                path: '/mgmt-api/browse',
                respond: () => ({
                    status: 403,
                    body: {code: 'no_access', message: "You do not have access to '/hdtms/private'."},
                }),
            },
            ...baseWorld(),
        ])
        renderApp({route: '/browse?path=%2Fhdtms%2Fprivate'})

        expect(await screen.findByText('You do not have access to this folder')).toBeInTheDocument()
        expect(screen.getByText("You do not have access to '/hdtms/private'.")).toBeInTheDocument()
        expect(screen.getByRole('link', {name: 'hdtms'})).toBeInTheDocument()
    })
})
