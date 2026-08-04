import {screen, within} from '@testing-library/react'
import {beforeEach, describe, expect, it} from 'vitest'
import {installApiMock} from '../api-mock'
import {renderApp, signIn} from '../render'
import {baseWorld} from '../world'
import {FIXTURE_PROJECT} from '../fixtures/project'

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

    it('reads the list from the new API while shares stay on the legacy one', async () => {
        // Pins the half-migrated state (#172) that this screen is in, so the
        // day shares move over, this fails and gets updated deliberately.
        const api = installApiMock(baseWorld())
        renderApp({route: '/projects'})
        await screen.findByText('My Projects')

        expect(api.matching('GET', '/api/v2/projects')).toHaveLength(1)
        expect(api.matching('GET', '/api/project/')).toHaveLength(0)
        expect(api.matching('GET', '/api/share/')).toHaveLength(1)
    })
})
