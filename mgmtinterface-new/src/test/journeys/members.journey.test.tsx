import {screen} from '@testing-library/react'
import {describe, expect, it} from 'vitest'
import {installApiMock} from '../api-mock'
import {renderApp, signIn} from '../render'
import {baseWorld} from '../world'
import {FIXTURE_PROJECT} from '../fixtures/project'

/**
 * #169 journey 4/6 — manage members: add one, remove one, save. The screen
 * applies add/remove as two sequential PATCH calls (see `ManageMembers.tsx`'s
 * `changeMembers` — the API has no atomic multi-op endpoint), so this is the
 * journey that pins that ordering and the two request bodies.
 */
describe('journey: manage members', () => {
    const name = FIXTURE_PROJECT.name
    // user2 is a real member of FIXTURE_PROJECT; user50 is in FIXTURE_USERS but not a member.
    const memberToRemove = 'user2'
    const memberToAdd = 'user50'

    it('adds one member and removes another in a single save', async () => {
        signIn(FIXTURE_PROJECT.owner, 'correct-horse')
        const recorder = installApiMock([
            ...baseWorld(),
            {
                method: 'PATCH',
                path: `/api/project/${name}/members/`,
                respond: () => ({body: {errno: 0, project: FIXTURE_PROJECT}}),
            },
        ])
        const {user} = renderApp({route: `/projects/${name}/members`})

        expect(await screen.findByText('Manage members')).toBeInTheDocument()

        // Remove: click the chip's remove button for memberToRemove.
        await user.click(screen.getByRole('button', {name: `Remove ${memberToRemove}`}))

        // Add: type into the picker and pick the matching candidate.
        await user.click(screen.getByPlaceholderText(''))
        await user.type(screen.getByPlaceholderText(''), memberToAdd)
        await user.click(await screen.findByRole('button', {name: new RegExp(memberToAdd)}))

        await user.click(screen.getByRole('button', {name: /save changes/i}))

        expect(await screen.findByRole('heading', {name})).toBeInTheDocument()

        const patches = recorder.matching('PATCH', `/api/project/${name}/members/`)
        expect(patches).toHaveLength(2)
        expect(patches[0].body).toEqual({
            op: 'add',
            members: [memberToAdd],
            user: FIXTURE_PROJECT.owner,
        })
        expect(patches[1].body).toEqual({
            op: 'remove',
            members: [memberToRemove],
            user: FIXTURE_PROJECT.owner,
        })
    })

    it('blocks a non-owner from managing members', async () => {
        // user2 is a member but not the owner.
        signIn('user2', 'correct-horse')
        installApiMock(baseWorld())
        renderApp({route: `/projects/${name}/members`})

        expect(await screen.findByText('Only the owner can manage members')).toBeInTheDocument()
    })
})
