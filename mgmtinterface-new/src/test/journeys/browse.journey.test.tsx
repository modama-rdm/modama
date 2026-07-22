import {screen} from '@testing-library/react'
import {describe, expect, it} from 'vitest'
import {installApiMock} from '../api-mock'
import {renderApp, signIn} from '../render'
import {baseWorld} from '../world'
import {FIXTURE_BROWSE} from '../fixtures/browse'

/**
 * #169 journey 6/6 — browse: a real production directory listing, chosen for
 * the exact edge cases #202 (space-containing folder names, `quotePath.ts`)
 * and MR!61 (the colored "Open in" badges) exist for.
 */
describe('journey: browse', () => {
    it('renders the listing, breadcrumb, and access badges for a real directory', async () => {
        signIn('user1', 'correct-horse')
        installApiMock(baseWorld())
        renderApp({route: `/browse?path=${encodeURIComponent(FIXTURE_BROWSE.path)}`})

        // A folder name with a space — the exact shape #202 exists to round-trip.
        const spacedEntry = FIXTURE_BROWSE.members.find(m => m.name.includes(' '))!
        expect(await screen.findByText(spacedEntry.name)).toBeInTheDocument()

        // Current folder's name — shown in the breadcrumb tail, the resource
        // bar title, and (since this folder is itself a project) the "in …" link.
        expect(screen.getAllByText(FIXTURE_BROWSE.info.name).length).toBeGreaterThanOrEqual(2)

        // MR!61's colored "Open in" badges — every access link this fixture carries.
        expect(screen.getAllByRole('link', {name: 'Nextcloud'}).length).toBeGreaterThan(0)
        expect(screen.getAllByRole('link', {name: 'SampleDB (ER-C-3)'}).length).toBeGreaterThan(0)
        expect(screen.getAllByRole('link', {name: 'ElabFTW (ER-C-3)'}).length).toBeGreaterThan(0)
    })

    it('shows the not-found state with a way back for a bad path', async () => {
        signIn('user1', 'correct-horse')
        // Listed before `...baseWorld()`: the mock's first-match-wins, and
        // baseWorld() already has its own GET /api/browse/ handler.
        installApiMock([
            {
                method: 'GET',
                path: '/api/browse/',
                respond: () => ({
                    body: {errno: 1, message: 'No such file or directory.', path: '/nope', backto: '/'},
                }),
            },
            ...baseWorld(),
        ])
        renderApp({route: '/browse?path=%2Fnope'})

        expect(await screen.findByText('Could not open this path')).toBeInTheDocument()
        expect(screen.getByText('No such file or directory.')).toBeInTheDocument()
        expect(screen.getByRole('link', {name: /go to \//i})).toBeInTheDocument()
    })
})
