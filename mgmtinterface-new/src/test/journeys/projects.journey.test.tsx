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
        signIn('user1')
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

    it('search finds a project by its words, in any order, across a dash (#113)', async () => {
        const {user} = renderApp({route: '/projects'})
        await screen.findByText('My Projects')

        // "ecspzl-u-aidtht" (user6's) is the only project with both words.
        await user.type(screen.getByPlaceholderText('Search projects…'), 'aidtht ecspzl')

        expect(screen.getByText('ecspzl-u-aidtht')).toBeInTheDocument()
        expect(screen.getAllByRole('row')).toHaveLength(2) // the header and the one project
        expect(screen.queryByText(FIXTURE_PROJECT.name)).not.toBeInTheDocument()
    })

    it('search finds projects by their owner', async () => {
        const {user} = renderApp({route: '/projects'})
        await screen.findByText('My Projects')

        // user39 owns "mws" and "vxb-i667" and nothing else.
        await user.type(screen.getByPlaceholderText('Search projects…'), 'user39')

        expect(screen.getByText('mws')).toBeInTheDocument()
        expect(screen.getByText('vxb-i667')).toBeInTheDocument()
        expect(screen.getAllByRole('row')).toHaveLength(3)
    })

    it('shows "No projects found" for a query that matches nothing', async () => {
        const {user} = renderApp({route: '/projects'})
        await screen.findByText('My Projects')

        await user.type(screen.getByPlaceholderText('Search projects…'), 'zzz-no-such-project')

        expect(await screen.findByText('No projects found')).toBeInTheDocument()
    })

    describe('sorting by column header (#75)', () => {
        const section = (title: string) => screen.getByText(title).closest('section')!
        // Each body row's first cell holds the name, then the path under it.
        const namesIn = (title: string) =>
            within(section(title)).getAllByRole('row').slice(1)
                .map(row => within(row).getAllByRole('cell')[0].firstElementChild!.textContent)
        const header = (title: string, name: string) =>
            within(section(title)).getByRole('columnheader', {name})

        // user1's own projects (FIXTURE_PROJECTS), newest first and A - Z.
        const OWN_RECENT = ['ljfmguujxbnwc', 'vjfgcwmvztbiwm', 'auypfzdy', 'rhjksukhr', 'fzhogcatlvnwg', 'wogx']
        const OWN_AZ = ['auypfzdy', 'fzhogcatlvnwg', 'ljfmguujxbnwc', 'rhjksukhr', 'vjfgcwmvztbiwm', 'wogx']

        it('lists projects newest first until a header is clicked', async () => {
            renderApp({route: '/projects'})
            await screen.findByText('My Projects')

            expect(namesIn('My Projects')).toEqual(OWN_RECENT)
            expect(header('My Projects', 'Last modified')).toHaveAttribute('aria-sort', 'descending')
            expect(header('My Projects', 'Project')).not.toHaveAttribute('aria-sort')
        })

        it('clicking "Project" sorts A - Z, and clicking it again Z - A', async () => {
            const {user} = renderApp({route: '/projects'})
            await screen.findByText('My Projects')
            const projectButton = () => within(header('My Projects', 'Project')).getByRole('button')

            await user.click(projectButton())
            expect(namesIn('My Projects')).toEqual(OWN_AZ)
            expect(header('My Projects', 'Project')).toHaveAttribute('aria-sort', 'ascending')
            expect(header('My Projects', 'Last modified')).not.toHaveAttribute('aria-sort')

            await user.click(projectButton())
            expect(namesIn('My Projects')).toEqual([...OWN_AZ].reverse())
            expect(header('My Projects', 'Project')).toHaveAttribute('aria-sort', 'descending')
        })

        it('a header sorts its own table only, by owner with ties by name', async () => {
            const {user} = renderApp({route: '/projects'})
            await screen.findByText('My Projects')

            await user.click(within(header('Shared with me', 'Owner')).getByRole('button'))

            // user6 before user15 (numbers compare as numbers), user15's two by name.
            expect(namesIn('Shared with me')).toEqual(['ecspzl-u-aidtht', 'eiidjlv-sywdhdjdnp', 'zpxe-wqb'])
            expect(header('Shared with me', 'Owner')).toHaveAttribute('aria-sort', 'ascending')
            // The other two tables keep their own order, newest first.
            expect(namesIn('My Projects')).toEqual(OWN_RECENT)
            expect(header('My Projects', 'Last modified')).toHaveAttribute('aria-sort', 'descending')
            expect(header('All projects', 'Last modified')).toHaveAttribute('aria-sort', 'descending')
            expect(header('All projects', 'Owner')).not.toHaveAttribute('aria-sort')

            await user.click(within(header('All projects', 'Owner')).getByRole('button'))

            // Bare numeric UIDs come before usernames.
            expect(namesIn('All projects').slice(0, 3)).toEqual(['yekr', 'xlzyki', 'ztr'])
            // Shared with me is still by owner, not moved by the other table.
            expect(header('Shared with me', 'Owner')).toHaveAttribute('aria-sort', 'ascending')
            expect(namesIn('My Projects')).toEqual(OWN_RECENT)
        })

        it('remembers each table\'s sort on the next visit', async () => {
            const first = renderApp({route: '/projects'})
            await screen.findByText('My Projects')
            await first.user.click(within(header('My Projects', 'Project')).getByRole('button'))
            await first.user.click(within(header('Shared with me', 'Owner')).getByRole('button'))
            first.unmount()

            renderApp({route: '/projects'})
            await screen.findByText('My Projects')
            expect(namesIn('My Projects')).toEqual(OWN_AZ)
            expect(header('My Projects', 'Project')).toHaveAttribute('aria-sort', 'ascending')
            expect(header('Shared with me', 'Owner')).toHaveAttribute('aria-sort', 'ascending')
            expect(header('All projects', 'Last modified')).toHaveAttribute('aria-sort', 'descending')
        })
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
