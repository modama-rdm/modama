import {screen} from '@testing-library/react'
import {describe, expect, it} from 'vitest'
import {installApiMock} from '../api-mock'
import {renderApp, signIn} from '../render'
import {baseWorld} from '../world'
import {FIXTURE_PROJECT} from '../fixtures/project'
import {makeFixtureShare} from '../fixtures/share'

/**
 * #169 journey 5/6 — ingest share: create, see connection credentials, close.
 * `share.ts` is a real capture from the test VM (see its header comment) — prod
 * had no active shares to capture. Opening a share moved to the new API (#172),
 * whose response also names the host to connect to; listing and closing are
 * still on the legacy endpoints.
 */
describe('journey: ingest share', () => {
    const name = FIXTURE_PROJECT.name
    const dataset = 'create by hand' // FIXTURE_DATASETS entry with immutable: false
    const route = `/projects/${name}/sessions/${encodeURIComponent(dataset)}/ingest`

    it('creates a share and shows connection credentials', async () => {
        signIn(FIXTURE_PROJECT.owner, 'correct-horse')
        const fixtureShare = makeFixtureShare()
        const recorder = installApiMock([
            ...baseWorld(),
            {
                method: 'POST',
                path: '/mgmt-api/ingest-shares',
                respond: () => ({
                    status: 201,
                    body: {
                        ...fixtureShare,
                        project: name,
                        session: dataset,
                        smb_host: 'ingest.example.test',
                        workgroup: 'LAB',
                    },
                }),
            },
        ])
        const {user} = renderApp({route})

        expect(await screen.findByRole('button', {name: /create ingest share/i})).toBeInTheDocument()
        await user.click(screen.getByRole('button', {name: /create ingest share/i}))

        expect(await screen.findByText('Ingest share is live')).toBeInTheDocument()
        expect(screen.getByText(fixtureShare.password)).toBeInTheDocument()
        // The host comes from the API now, not from a constant in the client.
        expect(
            screen.getByText(`\\\\ingest.example.test\\${fixtureShare.share_name}`),
        ).toBeInTheDocument()
        expect(screen.getByText(`LAB\\${fixtureShare.username}`)).toBeInTheDocument()
        const [call] = recorder.matching('POST', '/mgmt-api/ingest-shares')
        expect(call.body).toEqual({project: name, session: dataset})
    })

    it('shows an already-open share with a note instead of a success toast', async () => {
        // 200 means this user already had a share for the session (the list
        // loaded before was stale). The details are right; "Done!" would not be.
        signIn(FIXTURE_PROJECT.owner, 'correct-horse')
        const fixtureShare = makeFixtureShare()
        installApiMock([
            ...baseWorld(),
            {
                method: 'POST',
                path: '/mgmt-api/ingest-shares',
                respond: () => ({
                    status: 200,
                    body: {
                        ...fixtureShare,
                        project: name,
                        session: dataset,
                        smb_host: 'ingest.example.test',
                        workgroup: 'LAB',
                    },
                }),
            },
        ])
        const {user} = renderApp({route})

        await user.click(await screen.findByRole('button', {name: /create ingest share/i}))

        expect(await screen.findByText('Ingest share is live')).toBeInTheDocument()
        expect(screen.getByRole('status')).toHaveTextContent(/already had an ingest share open/i)
        expect(screen.queryByText('Done!')).not.toBeInTheDocument()
    })

    it("shows the API's reason when a share cannot be opened", async () => {
        signIn(FIXTURE_PROJECT.owner, 'correct-horse')
        installApiMock([
            ...baseWorld(),
            {
                method: 'POST',
                path: '/mgmt-api/ingest-shares',
                respond: () => ({
                    status: 502,
                    body: {
                        code: 'share_service_unavailable',
                        message: 'The ingest share service is unavailable. Please try again shortly.',
                    },
                }),
            },
        ])
        const {user} = renderApp({route})

        await user.click(await screen.findByRole('button', {name: /create ingest share/i}))

        expect(
            await screen.findByText(
                'The ingest share service is unavailable. Please try again shortly.',
            ),
        ).toBeInTheDocument()
        expect(screen.getByRole('button', {name: /create ingest share/i})).toBeInTheDocument()
    })

    it('closes an active share and returns to the create state', async () => {
        signIn(FIXTURE_PROJECT.owner, 'correct-horse')
        const fixtureShare = makeFixtureShare({dest_path: `/data/${FIXTURE_PROJECT.relpath}/${dataset}`})
        const recorder = installApiMock([
            ...baseWorld({shares: {[fixtureShare.share_id]: fixtureShare}}),
            {
                method: 'DELETE',
                path: `/api/share/${fixtureShare.share_id}/`,
                respond: () => ({body: {status: 'ok'}}),
            },
        ])
        const {user} = renderApp({route})

        expect(await screen.findByText('Ingest share is live')).toBeInTheDocument()
        await user.click(screen.getByRole('button', {name: /close share/i}))

        expect(await screen.findByRole('button', {name: /create ingest share/i})).toBeInTheDocument()
        expect(recorder.matching('DELETE', `/api/share/${fixtureShare.share_id}/`)).toHaveLength(1)
    })

    it('blocks a non-member from managing the ingest share', async () => {
        signIn('user50', 'correct-horse') // not owner, not a member of FIXTURE_PROJECT
        installApiMock(baseWorld())
        renderApp({route})

        expect(
            await screen.findByText('Only project members can manage ingest shares'),
        ).toBeInTheDocument()
    })
})
