import {screen} from '@testing-library/react'
import {beforeEach, describe, expect, it} from 'vitest'
import {installApiMock} from '../api-mock'
import {renderApp, signIn} from '../render'
import {baseWorld} from '../world'
import {FIXTURE_PROJECT} from '../fixtures/project'

/**
 * #169 journey 3/6 — create a session, then seal one. Both mutations use the
 * "errno-can't-discriminate" pattern (`NewSession.tsx` / `SealSession.tsx`) —
 * this is the journey that exercises that response shape through the real UI.
 */
describe('journey: create and seal a session', () => {
    const name = FIXTURE_PROJECT.name

    beforeEach(() => {
        signIn(FIXTURE_PROJECT.owner, 'correct-horse')
    })

    it('creates a session and returns to the project', async () => {
        const recorder = installApiMock([
            ...baseWorld(),
            {
                method: 'POST',
                path: `/api/project/${name}/dataset/`,
                respond: () => ({body: {errno: 0}}),
            },
        ])
        const {user} = renderApp({route: `/projects/${name}/sessions/new`})

        const input = await screen.findByLabelText('Session name')
        await user.clear(input)
        await user.type(input, '2026-07-22-alignment_run')
        await user.click(screen.getByRole('button', {name: /create session/i}))

        expect(await screen.findByRole('heading', {name})).toBeInTheDocument()
        const [call] = recorder.matching('POST', `/api/project/${name}/dataset/`)
        expect(call.body).toEqual({name: '2026-07-22-alignment_run', owner: FIXTURE_PROJECT.owner})
    })

    it('shows the backend error and stays on the form when creation fails', async () => {
        installApiMock([
            ...baseWorld(),
            {
                method: 'POST',
                path: `/api/project/${name}/dataset/`,
                respond: () => ({body: {errno: 1, message: 'A session with that name already exists.'}}),
            },
        ])
        const {user} = renderApp({route: `/projects/${name}/sessions/new`})

        await screen.findByLabelText('Session name')
        await user.click(screen.getByRole('button', {name: /create session/i}))

        expect(await screen.findByText('A session with that name already exists.')).toBeInTheDocument()
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
