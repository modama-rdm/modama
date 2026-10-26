import {screen} from '@testing-library/react'
import {beforeEach, describe, expect, it} from 'vitest'
import {installApiMock} from '../api-mock'
import {renderApp, signIn} from '../render'
import {baseWorld} from '../world'
import {FIXTURE_PROJECT} from '../fixtures/project'

/**
 * Journey — create a project. Creating moved to the new API (#172), which is
 * idempotent: 201 for a new project, 200 for one the user already owns, 409
 * for someone else's. The login journey only checks that `/projects/new`
 * renders; this one drives the form.
 */
describe('journey: new project', () => {
    const owner = FIXTURE_PROJECT.owner
    const created = {
        ...FIXTURE_PROJECT,
        name: 'ghost_imaging_2026',
        relpath: 'adhoc/ghost_imaging_2026',
        members: [owner],
        usage_bytes: 0,
    }

    beforeEach(() => {
        signIn(owner)
    })

    it('creates a project and opens it', async () => {
        const recorder = installApiMock([
            // The details screen reads the new project back; it is not in the
            // captured fixtures, so answer for it ahead of the base world.
            {
                method: 'GET',
                path: `/mgmt-api/projects/${created.name}`,
                respond: () => ({body: {...created, usage_bytes: null, sessions: []}}),
            },
            {
                method: 'GET',
                path: `/mgmt-api/projects/${created.name}/usage`,
                respond: () => ({body: {usage_bytes: 0}}),
            },
            ...baseWorld(),
            {
                method: 'POST',
                path: '/mgmt-api/projects',
                respond: () => ({status: 201, body: created}),
            },
        ])
        const {user} = renderApp({route: '/projects/new'})

        await user.type(await screen.findByLabelText(/project name/i), created.name)
        await user.click(screen.getByRole('button', {name: /create project/i}))

        expect(await screen.findByRole('heading', {name: created.name})).toBeInTheDocument()
        expect(screen.getByText(`Project "${created.name}" created.`)).toBeInTheDocument()
        const [call] = recorder.matching('POST', '/mgmt-api/projects')
        // No `owner`: the session cookie says who is creating it.
        expect(call.body).toEqual({name: created.name, members: []})
    })

    it('says so on the form when the user already owns a project of that name', async () => {
        // The API answers 200 with the existing project. That is neither a
        // success nor an error for the person at the form: they may have meant
        // a new name, so the form stays and offers the existing project.
        installApiMock([
            ...baseWorld(),
            {
                method: 'POST',
                path: '/mgmt-api/projects',
                respond: () => ({status: 200, body: FIXTURE_PROJECT}),
            },
        ])
        const {user} = renderApp({route: '/projects/new'})

        await user.type(await screen.findByLabelText(/project name/i), FIXTURE_PROJECT.name)
        await user.click(screen.getByRole('button', {name: /create project/i}))

        const notice = await screen.findByRole('status')
        expect(notice).toHaveTextContent(/you already own a project named/i)
        expect(notice).toHaveTextContent(FIXTURE_PROJECT.name)
        // No "Done!" toast and no redirect: the form is still there.
        expect(screen.queryByText('Done!')).not.toBeInTheDocument()
        expect(screen.getByLabelText(/project name/i)).toBeInTheDocument()

        await user.click(screen.getByRole('link', {name: /open the existing project/i}))
        expect(
            await screen.findByRole('heading', {name: FIXTURE_PROJECT.name}),
        ).toBeInTheDocument()
    })

    it("shows the API's reason and stays on the form for someone else's name", async () => {
        const recorder = installApiMock([
            ...baseWorld(),
            {
                method: 'POST',
                path: '/mgmt-api/projects',
                respond: () => ({
                    status: 409,
                    body: {
                        code: 'project_exists',
                        message: "A project named 'taken' already exists and belongs to user9.",
                    },
                }),
            },
        ])
        const {user} = renderApp({route: '/projects/new'})

        await user.type(await screen.findByLabelText(/project name/i), 'taken')
        await user.click(screen.getByRole('button', {name: /create project/i}))

        expect(
            await screen.findByText("A project named 'taken' already exists and belongs to user9."),
        ).toBeInTheDocument()
        expect(screen.getByLabelText(/project name/i)).toBeInTheDocument()
        expect(recorder.matching('POST', '/mgmt-api/projects')).toHaveLength(1)
    })

    it('explains the name rule before sending anything', async () => {
        const recorder = installApiMock(baseWorld())
        const {user} = renderApp({route: '/projects/new'})

        await user.type(await screen.findByLabelText(/project name/i), 'Ghost Imaging')
        await user.click(screen.getByRole('button', {name: /create project/i}))

        // The help text under the field states the rule too; match the error.
        expect(
            await screen.findByText(/can only use lowercase letters, digits/i),
        ).toBeInTheDocument()
        expect(recorder.matching('POST', '/mgmt-api/projects')).toHaveLength(0)
    })
})
