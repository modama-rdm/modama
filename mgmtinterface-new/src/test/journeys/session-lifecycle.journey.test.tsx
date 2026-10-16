import {screen} from '@testing-library/react'
import {beforeEach, describe, expect, it} from 'vitest'
import {installApiMock} from '../api-mock'
import {renderApp, signIn} from '../render'
import {baseWorld} from '../world'
import {FIXTURE_PROJECT} from '../fixtures/project'

/**
 * #169 journey 3/6 — create a session, then seal one. Creating moved to the
 * new API (#172): real status codes, 201 for a new session and 200 for one
 * that already existed. Sealing still uses the legacy "errno-can't-
 * discriminate" pattern (`SealSession.tsx`), which this journey keeps
 * exercising through the real UI.
 */
describe('journey: create and seal a session', () => {
    const name = FIXTURE_PROJECT.name

    beforeEach(() => {
        signIn(FIXTURE_PROJECT.owner, 'correct-horse')
    })

    const createSession = `/mgmt-api/projects/${name}/sessions`

    it('creates a session and returns to the project', async () => {
        const recorder = installApiMock([
            ...baseWorld(),
            {
                method: 'POST',
                path: createSession,
                respond: ({body}) => ({
                    status: 201,
                    body: {
                        name: (body as {name: string}).name,
                        immutable: false,
                        ctime: 1784720092,
                        mtime: 1784720092,
                    },
                }),
            },
        ])
        const {user} = renderApp({route: `/projects/${name}/sessions/new`})

        const input = await screen.findByLabelText('Session name')
        await user.clear(input)
        await user.type(input, '2026-07-22-alignment_run')
        await user.click(screen.getByRole('button', {name: /create session/i}))

        expect(await screen.findByRole('heading', {name})).toBeInTheDocument()
        expect(screen.getByText('Session "2026-07-22-alignment_run" created.')).toBeInTheDocument()
        const [call] = recorder.matching('POST', createSession)
        // No `owner`: the session cookie says who is creating it.
        expect(call.body).toEqual({name: '2026-07-22-alignment_run'})
    })

    it('says so on the form when the session already exists', async () => {
        // The API is idempotent: a repeat answers 200 with the existing session.
        // The form stays, with a way back to the project, instead of a "Done!".
        installApiMock([
            ...baseWorld(),
            {
                method: 'POST',
                path: createSession,
                respond: () => ({
                    status: 200,
                    body: {name: 'create by hand', immutable: false, ctime: 1, mtime: 2},
                }),
            },
        ])
        const {user} = renderApp({route: `/projects/${name}/sessions/new`})

        const input = await screen.findByLabelText('Session name')
        await user.clear(input)
        await user.type(input, 'create by hand')
        await user.click(screen.getByRole('button', {name: /create session/i}))

        const notice = await screen.findByRole('status')
        expect(notice).toHaveTextContent(/already exists in this project/i)
        expect(notice).toHaveTextContent('create by hand')
        expect(screen.queryByText('Done!')).not.toBeInTheDocument()
        expect(screen.getByLabelText('Session name')).toBeInTheDocument()

        await user.click(screen.getByRole('link', {name: /go back to the project/i}))
        expect(await screen.findByRole('heading', {name})).toBeInTheDocument()
    })

    it('shows the backend error and stays on the form when creation fails', async () => {
        installApiMock([
            ...baseWorld(),
            {
                method: 'POST',
                path: createSession,
                respond: () => ({
                    status: 400,
                    body: {
                        code: 'session_rejected',
                        message: 'mkdir: cannot create directory: Permission denied',
                    },
                }),
            },
        ])
        const {user} = renderApp({route: `/projects/${name}/sessions/new`})

        await screen.findByLabelText('Session name')
        await user.click(screen.getByRole('button', {name: /create session/i}))

        expect(
            await screen.findByText('mkdir: cannot create directory: Permission denied'),
        ).toBeInTheDocument()
        expect(screen.getByLabelText('Session name')).toBeInTheDocument()
    })

    it('seals an open session and returns to the project', async () => {
        const dataset = 'create by hand' // FIXTURE_DATASETS entry with immutable: false
        const recorder = installApiMock([
            ...baseWorld(),
            {
                method: 'POST',
                path: `/api/project/${name}/dataset/seal/`,
                respond: () => ({body: {errno: 0}}),
            },
        ])
        const {user} = renderApp({route: `/projects/${name}/sessions/${encodeURIComponent(dataset)}/seal`})

        await screen.findByText('Seal this session?')
        await user.click(screen.getByRole('button', {name: /seal session permanently/i}))

        expect(await screen.findByRole('heading', {name})).toBeInTheDocument()
        const [call] = recorder.matching('POST', `/api/project/${name}/dataset/seal/`)
        expect(call.body).toEqual({name: dataset, user: FIXTURE_PROJECT.owner})
    })
})
