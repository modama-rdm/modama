import {screen, within} from '@testing-library/react'
import {beforeEach, describe, expect, it} from 'vitest'
import {installApiMock} from '../api-mock'
import {renderApp, signIn} from '../render'
import {baseWorld} from '../world'
import {FIXTURE_PROJECT, FIXTURE_DATASETS} from '../fixtures/project'

/**
 * #113 — a project's sessions table has a search box like the projects list's.
 * The 16 sessions are `FIXTURE_DATASETS`, captured from prod, with names that
 * mix spaces and dashes ("from ingest", "2022-06-14-test-from-ingest-gui").
 */
describe('journey: searching a project\'s sessions', () => {
    const route = `/projects/${FIXTURE_PROJECT.name}`
    const placeholder = 'Search sessions…'

    beforeEach(() => {
        signIn(FIXTURE_PROJECT.owner)
        installApiMock(baseWorld())
    })

    const openProject = async () => {
        const app = renderApp({route})
        await screen.findByRole('heading', {name: FIXTURE_PROJECT.name})
        return app
    }
    // The session name is the first line of the first cell; a description's
    // first line may follow it.
    const sessionNames = () =>
        within(screen.getByRole('table')).getAllByRole('row').slice(1)
            .map(row => within(row).getAllByRole('cell')[0].firstElementChild?.textContent)

    it('narrows the table to the sessions with every word, across spaces and dashes', async () => {
        const {user} = await openProject()
        expect(sessionNames()).toHaveLength(FIXTURE_DATASETS.length)

        await user.type(screen.getByPlaceholderText(placeholder), 'ingest')
        // Newest-modified first still: the sort applies to what matches.
        expect(sessionNames()).toEqual([
            '2023-11-06-ingest-stuff', '2022-06-14-test-from-ingest-gui', 'from ingest',
        ])

        await user.type(screen.getByPlaceholderText(placeholder), ' test')
        expect(sessionNames()).toEqual(['2022-06-14-test-from-ingest-gui'])
    })

    it('sorts what matches when a header is clicked', async () => {
        const {user} = await openProject()

        await user.type(screen.getByPlaceholderText(placeholder), 'ingest')
        await user.click(within(screen.getByRole('columnheader', {name: 'Session'})).getByRole('button'))

        expect(sessionNames()).toEqual([
            '2022-06-14-test-from-ingest-gui', '2023-11-06-ingest-stuff', 'from ingest',
        ])
    })

    it('says what matched nothing, and the table is back once the query is cleared', async () => {
        const {user} = await openProject()

        await user.type(screen.getByPlaceholderText(placeholder), 'zzz')

        expect(screen.getByText('No sessions found')).toBeInTheDocument()
        expect(screen.getByText('Nothing matches "zzz".')).toBeInTheDocument()
        expect(screen.queryByRole('table')).not.toBeInTheDocument()
        // Not the card for a project without sessions.
        expect(screen.queryByText('No sessions yet')).not.toBeInTheDocument()
        expect(screen.queryByRole('button', {name: 'Create first session'})).not.toBeInTheDocument()

        await user.clear(screen.getByPlaceholderText(placeholder))
        expect(sessionNames()).toHaveLength(FIXTURE_DATASETS.length)
        expect(screen.queryByText('No sessions found')).not.toBeInTheDocument()
    })

    it('has no search box on a project without sessions', async () => {
        // "wogx" is user1's too; the world answers it with no sessions.
        renderApp({route: '/projects/wogx'})
        await screen.findByRole('heading', {name: 'wogx'})

        expect(screen.getByText('No sessions yet')).toBeInTheDocument()
        expect(screen.queryByPlaceholderText(placeholder)).not.toBeInTheDocument()
    })

    it('starts empty on the next visit', async () => {
        const first = await openProject()
        await first.user.type(screen.getByPlaceholderText(placeholder), 'ingest')
        first.unmount()

        await openProject()
        expect(screen.getByPlaceholderText(placeholder)).toHaveValue('')
        expect(sessionNames()).toHaveLength(FIXTURE_DATASETS.length)
    })
})
