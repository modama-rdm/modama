import {screen, within} from '@testing-library/react'
import {beforeEach, describe, expect, it} from 'vitest'
import {installApiMock} from '../api-mock'
import {renderApp, signIn} from '../render'
import {baseWorld} from '../world'
import {FIXTURE_PROJECT} from '../fixtures/project'
import {makeFixtureShare} from '../fixtures/share'

/**
 * #75 — a project's sessions table sorts by clicking a column header, like the
 * projects list. The 16 sessions are `FIXTURE_DATASETS`, captured from prod,
 * with "Last modified" (mtime) spanning four years. Their `ctime` is the same
 * minute for all of them, which is why the table has no "Created" column.
 */
describe('journey: sorting the sessions table', () => {
    const route = `/projects/${FIXTURE_PROJECT.name}`

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
    const header = (name: string) => screen.getByRole('columnheader', {name})
    const clickHeader = async (user: {click: (el: Element) => Promise<void>}, name: string) =>
        user.click(within(header(name)).getByRole('button'))

    it('starts newest-modified first, with Last modified and no Created column', async () => {
        await openProject()

        expect(sessionNames().slice(0, 3)).toEqual([
            '2026-02-23-rdm-challenge-modama-agent', 'testsession', '2024-11-12-biasing-test',
        ])
        expect(header('Last modified')).toHaveAttribute('aria-sort', 'descending')
        expect(screen.queryByRole('columnheader', {name: 'Created'})).not.toBeInTheDocument()
        expect(screen.getAllByRole('columnheader').map(th => th.textContent))
            .toEqual(['Session', 'Last modified', 'Status', ''])
    })

    it('clicking "Session" sorts A - Z with numbers in order, and clicking it again Z - A', async () => {
        const {user} = await openProject()

        await clickHeader(user, 'Session')
        expect(sessionNames().slice(0, 3)).toEqual([
            '2022-03-22 my first session', '2022-04-01 Debug HDF5 issues', '2022-06-02 another testsession',
        ])
        expect(sessionNames().at(-1)).toBe('testsession')
        expect(header('Session')).toHaveAttribute('aria-sort', 'ascending')

        await clickHeader(user, 'Session')
        expect(sessionNames()[0]).toBe('testsession')
        expect(header('Session')).toHaveAttribute('aria-sort', 'descending')
    })

    it('"Status" puts the session with an ingest share first and the sealed ones last', async () => {
        const share = makeFixtureShare({project: FIXTURE_PROJECT.name, session: 'create by hand'})
        installApiMock(baseWorld({shares: [share]}))
        const {user} = await openProject()

        await clickHeader(user, 'Status')

        const names = sessionNames()
        expect(names[0]).toBe('create by hand')
        expect(names.slice(-3)).toEqual(['2022-06-14-datendemo', 'demo session', 'testsession'])
    })

    it('remembers the sort on the next visit, apart from the projects list', async () => {
        const first = await openProject()
        await clickHeader(first.user, 'Session')
        first.unmount()

        const second = await openProject()
        expect(header('Session')).toHaveAttribute('aria-sort', 'ascending')
        second.unmount()

        renderApp({route: '/projects'})
        await screen.findByText('My Projects')
        const own = screen.getByText('My Projects').closest('section')!
        expect(within(own).getByRole('columnheader', {name: 'Last modified'}))
            .toHaveAttribute('aria-sort', 'descending')
    })
})
