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
 * had no active shares to capture. Opening, listing and closing are all on the
 * new API (#172); every share names its project, session and the host to
 * connect to, so the screen matches on those rather than on a path.
 */
describe('journey: ingest share', () => {
    const name = FIXTURE_PROJECT.name
    const dataset = 'create by hand' // FIXTURE_DATASETS entry with immutable: false
    const route = `/projects/${name}/sessions/${encodeURIComponent(dataset)}/ingest`
    /** The fixture share, as opened for this project and session. */
    const openShare = () => makeFixtureShare({project: name, session: dataset})

    it('creates a share and shows connection credentials', async () => {
        signIn(FIXTURE_PROJECT.owner)
        const fixtureShare = openShare()
        const recorder = installApiMock([
            ...baseWorld(),
            {
                method: 'POST',
                path: '/mgmt-api/ingest-shares',
                respond: () => ({
                    status: 201,
                    body: {...fixtureShare, smb_host: 'ingest.example.test', workgroup: 'LAB'},
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
        signIn(FIXTURE_PROJECT.owner)
        installApiMock([
            ...baseWorld(),
            {
                method: 'POST',
                path: '/mgmt-api/ingest-shares',
                respond: () => ({status: 200, body: openShare()}),
            },
        ])
        const {user} = renderApp({route})

        await user.click(await screen.findByRole('button', {name: /create ingest share/i}))

        expect(await screen.findByText('Ingest share is live')).toBeInTheDocument()
        expect(screen.getByRole('status')).toHaveTextContent(/already have an ingest share/i)
        expect(screen.queryByText('Done!')).not.toBeInTheDocument()
    })

    it("shows the API's reason when a share cannot be opened", async () => {
        signIn(FIXTURE_PROJECT.owner)
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

    it('shows an open share from the list with the host it names', async () => {
        // The list is the user's shares with project and session on each; the
        // one for this session is shown as live, with its own host.
        signIn(FIXTURE_PROJECT.owner)
        const mine = openShare()
        const other = makeFixtureShare({share_id: 'other', session: 'demo session', smb_host: 'elsewhere.test'})
        installApiMock(baseWorld({shares: [other, mine]}))
        renderApp({route})

        expect(await screen.findByText('Ingest share is live')).toBeInTheDocument()
        expect(screen.getByText(`\\\\${mine.smb_host}\\${mine.share_name}`)).toBeInTheDocument()
    })

    it('closes an active share and returns to the create state', async () => {
        signIn(FIXTURE_PROJECT.owner)
        const fixtureShare = openShare()
        const close = `/mgmt-api/ingest-shares/${fixtureShare.share_id}`
        const recorder = installApiMock([
            ...baseWorld({shares: [fixtureShare]}),
            {method: 'DELETE', path: close, respond: () => ({status: 204, body: null})},
        ])
        const {user} = renderApp({route})

        expect(await screen.findByText('Ingest share is live')).toBeInTheDocument()
        await user.click(screen.getByRole('button', {name: /stop ingest/i}))

        expect(await screen.findByRole('button', {name: /create ingest share/i})).toBeInTheDocument()
        expect(screen.getByText(`Ingest share for "${dataset}" stopped.`)).toBeInTheDocument()
        expect(recorder.matching('DELETE', close)).toHaveLength(1)
    })

    it('treats a share that is gone already as closed', async () => {
        // Closed from another tab, or by the owner elsewhere: the API answers
        // 404, and the state the user wanted is the one to show.
        signIn(FIXTURE_PROJECT.owner)
        const fixtureShare = openShare()
        installApiMock([
            ...baseWorld({shares: [fixtureShare]}),
            {
                method: 'DELETE',
                path: `/mgmt-api/ingest-shares/${fixtureShare.share_id}`,
                respond: () => ({
                    status: 404,
                    body: {code: 'share_not_found', message: 'There is no open ingest share.'},
                }),
            },
        ])
        const {user} = renderApp({route})

        await user.click(await screen.findByRole('button', {name: /stop ingest/i}))

        expect(await screen.findByRole('button', {name: /create ingest share/i})).toBeInTheDocument()
        expect(screen.getByText(`Ingest share for "${dataset}" was already stopped.`)).toBeInTheDocument()
    })

    it("shows the API's reason when a share cannot be closed", async () => {
        signIn(FIXTURE_PROJECT.owner)
        const fixtureShare = openShare()
        installApiMock([
            ...baseWorld({shares: [fixtureShare]}),
            {
                method: 'DELETE',
                path: `/mgmt-api/ingest-shares/${fixtureShare.share_id}`,
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

        await user.click(await screen.findByRole('button', {name: /stop ingest/i}))

        expect(
            await screen.findByText('The ingest share service is unavailable. Please try again shortly.'),
        ).toBeInTheDocument()
        // Still live: nothing was closed.
        expect(screen.getByRole('button', {name: /stop ingest/i})).toBeInTheDocument()
    })

    it('blocks a non-member from managing the ingest share', async () => {
        signIn('user50') // not owner, not a member of FIXTURE_PROJECT
        installApiMock(baseWorld())
        renderApp({route})

        expect(
            await screen.findByText('Only project members can manage ingest shares'),
        ).toBeInTheDocument()
    })
})
