import {screen, within} from '@testing-library/react'
import {beforeEach, describe, expect, it} from 'vitest'
import {installApiMock} from '../api-mock'
import {renderApp, signIn} from '../render'
import {baseWorld, browseReply} from '../world'
import {FIXTURE_BROWSE} from '../fixtures/browse'

/**
 * Journey — where a page opens. A link to another page opens it at the top,
 * not at the scroll offset of the page you came from; Back returns you to
 * where you were. Things that change a page in place (sorting a table, "Load
 * more") leave the page where it is. jsdom cannot scroll, so `window.scrollTo`
 * and `scrollY` are a stand-in that remembers the position (see `setup.ts`).
 */
describe('journey: where a page opens', () => {
    /** A user who has scrolled well down the page, e.g. to reach the footer. */
    const SCROLLED = 1500

    beforeEach(() => {
        signIn('user1')
        installApiMock(baseWorld())
    })

    async function scrolledDownProjects() {
        const app = renderApp({route: '/projects'})
        await screen.findByText('My Projects')
        window.scrollTo(0, SCROLLED)
        return app
    }

    it('opens the changelog at the top when the footer version is clicked', async () => {
        const {user} = await scrolledDownProjects()

        await user.click(screen.getByRole('link', {name: `v${__APP_VERSION__}`}))

        expect(await screen.findByRole('heading', {name: 'Changelog'})).toBeInTheDocument()
        expect(window.scrollY).toBe(0)
    })

    it('goes back to where you were on the projects list', async () => {
        const {user, router} = await scrolledDownProjects()
        await user.click(screen.getByRole('link', {name: `v${__APP_VERSION__}`}))
        await screen.findByRole('heading', {name: 'Changelog'})
        expect(window.scrollY).toBe(0)

        await router.navigate(-1)

        expect(await screen.findByText('My Projects')).toBeInTheDocument()
        expect(window.scrollY).toBe(SCROLLED)
    })

    it('keeps the page where it is when a table is sorted', async () => {
        const {user} = await scrolledDownProjects()
        const section = screen.getByText('My Projects').closest('section')!

        await user.click(within(within(section).getByRole('columnheader', {name: 'Project'})).getByRole('button'))

        expect(within(section).getByRole('columnheader', {name: 'Project'})).toHaveAttribute('aria-sort', 'ascending')
        expect(window.scrollY).toBe(SCROLLED)
    })

    it('keeps the page where it is on "Load more" in the data browser', async () => {
        installApiMock([
            {method: 'GET', path: '/mgmt-api/browse', respond: ({params}) => browseReply(params, 4)},
            ...baseWorld(),
        ])
        const {user} = renderApp({route: `/browse?path=${encodeURIComponent(FIXTURE_BROWSE.path)}`})
        expect(await screen.findByText(`4 of ${FIXTURE_BROWSE.total}`)).toBeInTheDocument()
        window.scrollTo(0, SCROLLED)

        await user.click(screen.getByRole('button', {name: /load more/i}))

        expect(await screen.findByText(`8 of ${FIXTURE_BROWSE.total}`)).toBeInTheDocument()
        expect(window.scrollY).toBe(SCROLLED)
    })
})
