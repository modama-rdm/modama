import {screen} from '@testing-library/react'
import {describe, expect, it} from 'vitest'
import {installApiMock} from '../api-mock'
import {renderApp, signIn} from '../render'
import {baseWorld} from '../world'
import {FIXTURE_PROJECT} from '../fixtures/project'

/**
 * #169 journey 4/6 — manage members: add one, remove one, save. Saving moved
 * to the new API (#172), which takes the complete list in one `PUT` and works
 * out what to add and remove itself, so this journey pins that there is one
 * request and that it carries the list as the user left it.
 */
describe('journey: manage members', () => {
    const name = FIXTURE_PROJECT.name
    const members = `/mgmt-api/projects/${name}/members`
    // user2 is a real member of FIXTURE_PROJECT; user50 is in FIXTURE_USERS but not a member.
    const memberToRemove = 'user2'
    const memberToAdd = 'user50'

    async function editMembers(user: ReturnType<typeof renderApp>['user']) {
        expect(await screen.findByText('Manage members')).toBeInTheDocument()

        // Remove: click the chip's remove button for memberToRemove.
        await user.click(screen.getByRole('button', {name: `Remove ${memberToRemove}`}))

        // Add: type into the picker and pick the matching candidate.
        await user.click(screen.getByPlaceholderText(''))
        await user.type(screen.getByPlaceholderText(''), memberToAdd)
        await user.click(await screen.findByRole('button', {name: new RegExp(memberToAdd)}))

        await user.click(screen.getByRole('button', {name: /save changes/i}))
    }

    it('adds one member and removes another in a single save', async () => {
        signIn(FIXTURE_PROJECT.owner, 'correct-horse')
        const recorder = installApiMock([
            ...baseWorld(),
            {
                method: 'PUT',
                path: members,
                respond: ({body}) => ({
                    body: {...FIXTURE_PROJECT, members: (body as {members: string[]}).members},
                }),
            },
        ])
        const {user} = renderApp({route: `/projects/${name}/members`})

        await editMembers(user)

        expect(await screen.findByRole('heading', {name})).toBeInTheDocument()
        expect(screen.getByText(`Members of "${name}" updated.`)).toBeInTheDocument()

        const puts = recorder.matching('PUT', members)
        expect(puts).toHaveLength(1)
        // The whole list as the user left it: the dropped member gone, the new
        // one in. The API computes the difference; no `op`, no `user`.
        const wanted = new Set([...FIXTURE_PROJECT.members.filter(m => m !== memberToRemove), memberToAdd])
        expect(new Set((puts[0].body as {members: string[]}).members)).toEqual(wanted)
        expect(puts[0].body).not.toHaveProperty('op')
    })

    it("shows the API's reason and keeps the form when the change is refused", async () => {
        signIn(FIXTURE_PROJECT.owner, 'correct-horse')
        installApiMock([
            ...baseWorld(),
            {
                method: 'PUT',
                path: members,
                respond: () => ({
                    status: 400,
                    body: {
                        code: 'members_partially_changed',
                        message: 'Added user50, but could not remove user2: Encountered errors when walking path',
                    },
                }),
            },
        ])
        const {user} = renderApp({route: `/projects/${name}/members`})

        await editMembers(user)

        // The form stays, with the list as the user wants it: saving again sends
        // the same list and the server applies only what is still missing.
        expect(
            await screen.findByText(/Added user50, but could not remove user2/),
        ).toBeInTheDocument()
        expect(screen.getByRole('button', {name: /save changes/i})).toBeInTheDocument()
        expect(screen.getByRole('button', {name: `Remove ${memberToAdd}`})).toBeInTheDocument()
    })

    it('blocks a non-owner from managing members', async () => {
        // user2 is a member but not the owner.
        signIn('user2', 'correct-horse')
        installApiMock(baseWorld())
        renderApp({route: `/projects/${name}/members`})

        expect(await screen.findByText('Only the owner can manage members')).toBeInTheDocument()
    })
})
