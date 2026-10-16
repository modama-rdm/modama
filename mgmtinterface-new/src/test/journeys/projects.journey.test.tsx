import {screen, within} from '@testing-library/react'
import {beforeEach, describe, expect, it} from 'vitest'
import {installApiMock} from '../api-mock'
import {renderApp, signIn} from '../render'
import {baseWorld} from '../world'
import {FIXTURE_PROJECT} from '../fixtures/project'
import {makeFixtureShare} from '../fixtures/share'

/**
 * #169 journey 2/6 — projects list: grouping, search, sort, and navigating
 * into a project. Exercises `lib/projectsUtil.ts` end-to-end through the real
 * screen rather than only at the unit level (`lib/projects.test.ts`).
 */
describe('journey: projects list', () => {
    beforeEach(() => {
        signIn('user1', 'correct-horse')
        installApiMock(baseWorld())
    })

    it('groups the signed-in user\'s own projects under "My Projects"', async () => {
        renderApp({route: '/projects'})

        const ownSection = (await screen.findByText('My Projects')).closest('section')!
        // FIXTURE_PROJECT ("vjfgcwmvztbiwm") is owned by user1 — it must show up here.
        expect(within(ownSection).getByText(FIXTURE_PROJECT.name)).toBeInTheDocument()
    })

    it('search narrows the list to a single matching project', async () => {
        const {user} = renderApp({route: '/projects'})
        await screen.findByText('My Projects')

        // "mws" is a real (scrubbed) project name unique enough for an exact match.
        await user.type(screen.getByPlaceholderText('Search projects…'), 'mws')

        expect(screen.getByText('mws')).toBeInTheDocument()
        expect(screen.queryByText(FIXTURE_PROJECT.name)).not.toBeInTheDocument()
    })

    it('shows "No projects found" for a query that matches nothing', async () => {
        const {user} = renderApp({route: '/projects'})
        await screen.findByText('My Projects')

        await user.type(screen.getByPlaceholderText('Search projects…'), 'zzz-no-such-project')

        expect(await screen.findByText('No projects found')).toBeInTheDocument()
    })

    it('clicking a project row navigates to its details page', async () => {
        const {user} = renderApp({route: '/projects'})
        await screen.findByText('My Projects')

        await user.click(screen.getByText(FIXTURE_PROJECT.name))

        expect(await screen.findByRole('heading', {name: FIXTURE_PROJECT.name})).toBeInTheDocument()
        expect(screen.getByText(`Project owned by ${FIXTURE_PROJECT.owner}`)).toBeInTheDocument()
    })

    it('reads the list and the shares from the new API and nothing from the legacy one', async () => {
        // Pins that this screen is fully off Flask (#172).
        const api = installApiMock(baseWorld())
        renderApp({route: '/projects'})
        await screen.findByText('My Projects')

        expect(api.matching('GET', '/mgmt-api/projects')).toHaveLength(1)
        expect(api.matching('GET', '/mgmt-api/ingest-shares')).toHaveLength(1)
        expect(api.calls.filter(c => c.path.startsWith('/api/'))).toHaveLength(0)
    })

    it('shows an active upload for each open share, by project and session', async () => {
        // The share names its project and session; nothing is parsed off a path.
        const share = makeFixtureShare({project: FIXTURE_PROJECT.name, session: 'create by hand'})
        installApiMock(baseWorld({shares: [share]}))
        const {user} = renderApp({route: '/projects'})

        const banner = (await screen.findByText('Active upload in progress')).closest('div')!.parentElement!
        expect(within(banner).getByText(FIXTURE_PROJECT.name)).toBeInTheDocument()
        expect(within(banner).getByText('create by hand')).toBeInTheDocument()

        await user.click(within(banner).getByRole('button', {name: /manage ingest/i}))
        expect(await screen.findByText('Ingest share is live')).toBeInTheDocument()
    })
})
