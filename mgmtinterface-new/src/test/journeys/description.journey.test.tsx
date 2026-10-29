import {screen, within} from '@testing-library/react'
import {describe, expect, it} from 'vitest'
import {installApiMock, type ApiHandler} from '../api-mock'
import {renderApp, signIn} from '../render'
import {baseWorld} from '../world'
import {FIXTURE_DATASETS, FIXTURE_PROJECT} from '../fixtures/project'

/**
 * #112 — descriptions, kept as README.md in the project or session folder.
 * The lists show the first line with the whole text on hover, the project page
 * renders the project's as Markdown, the search covers them, and the owner and
 * the members change them on their own screen. The texts are the hand-written
 * ones in the fixtures (see `fixtures/README.md`).
 */
describe('journey: descriptions', () => {
    const name = FIXTURE_PROJECT.name
    const projectDescription = FIXTURE_PROJECT.description!
    const session = FIXTURE_DATASETS.find((d) => d.name === '2022-03-22 my first session')!
    const projectPut = `/mgmt-api/projects/${name}/description`
    const sessionPut = `/mgmt-api/projects/${name}/sessions/${encodeURIComponent(session.name)}/description`
    // user1 owns the project, user2 is a member, user50 is neither.
    const outsider = 'user50'

    const echoPut = (path: string, base: object): ApiHandler => ({
        method: 'PUT',
        path,
        respond: ({body}) => ({
            body: {...base, description: (body as {description: string}).description || null},
        }),
    })

    const rowOf = (text: string) => screen.getByText(text).closest('tr')!

    describe('projects list', () => {
        it('shows the first line under the name and the whole text on hover', async () => {
            signIn('user1')
            installApiMock(baseWorld())
            renderApp({route: '/projects'})
            await screen.findByText('My Projects')

            // The heading's text, without the `#`.
            const digest = within(rowOf(name)).getByText('Data management dogfooding')
            expect(digest).toHaveAttribute('title', projectDescription)
            // A project without a description has only its name and path.
            expect(within(rowOf('mws')).getAllByRole('cell')[0].children).toHaveLength(2)
        })

        it('finds a project by words of its description', async () => {
            signIn('user1')
            installApiMock(baseWorld())
            const {user} = renderApp({route: '/projects'})
            await screen.findByText('My Projects')

            // "nanowires" and "titan" occur only in wys_ztv0's description.
            await user.type(screen.getByPlaceholderText('Search projects…'), 'titan nanowires')

            expect(screen.getByText('wys_ztv0')).toBeInTheDocument()
            expect(screen.getAllByRole('row')).toHaveLength(2) // the header and the one project
        })
    })

    describe('project page', () => {
        it('renders the description as Markdown, with Edit for the owner', async () => {
            signIn(FIXTURE_PROJECT.owner)
            installApiMock(baseWorld())
            renderApp({route: `/projects/${name}`})

            // `#` is shifted down, so it does not compete with the page's own h1.
            expect(await screen.findByRole('heading', {level: 3, name: 'Data management dogfooding'}))
                .toBeInTheDocument()
            expect(screen.getByText('ER-C data management').tagName).toBe('STRONG')
            expect(screen.getByText('Sessions from the agent and the SMB drop').tagName).toBe('LI')
            const link = screen.getByRole('link', {name: 'handbook'})
            expect(link).toHaveAttribute('href', 'https://example.org/rdm-handbook')
            expect(link).toHaveAttribute('rel', 'noopener noreferrer')
            expect(screen.getByRole('button', {name: 'Edit'})).toBeInTheDocument()
        })

        it('shows the description but no way to change it to someone outside the project', async () => {
            signIn(outsider)
            installApiMock(baseWorld())
            renderApp({route: `/projects/${name}`})

            expect(await screen.findByRole('heading', {name: 'Data management dogfooding'})).toBeInTheDocument()
            expect(screen.queryByRole('button', {name: 'Edit'})).not.toBeInTheDocument()
            expect(screen.queryByRole('link', {name: /edit the description/i})).not.toBeInTheDocument()
        })

        it('offers a member to add a description where there is none', async () => {
            signIn('user1')
            installApiMock(baseWorld())
            const {user} = renderApp({route: '/projects/rhjksukhr'}) // user1's, no description

            await user.click(await screen.findByRole('link', {name: 'Add a description'}))

            expect(await screen.findByRole('heading', {name: 'Project description'})).toBeInTheDocument()
            expect(screen.getByLabelText('Description')).toHaveValue('')
        })

        it('does not render raw HTML from a description', async () => {
            signIn('user1')
            installApiMock([
                {
                    method: 'GET',
                    path: `/mgmt-api/projects/${name}`,
                    respond: () => ({
                        body: {
                            ...FIXTURE_PROJECT,
                            usage_bytes: null,
                            sessions: [],
                            description: 'Before <img src=x onerror="alert(1)"> after [x](javascript:alert(1))',
                        },
                    }),
                },
                ...baseWorld(),
            ])
            renderApp({route: `/projects/${name}`})

            // Wait for the rendered text: until the renderer has loaded, the
            // page shows the description as it is.
            const paragraph = (await screen.findByText('x')).closest('p')!
            expect(paragraph.querySelector('img')).toBeNull()
            expect(paragraph.querySelector('[onerror]')).toBeNull()
            // Shown as the text it is, not as markup.
            expect(paragraph.textContent).toContain('<img src=x onerror="alert(1)">')
            // A link whose target is dropped as unsafe is plain text.
            expect(within(paragraph).getByText('x').closest('a')).toBeNull()
        })
    })

    describe('editing', () => {
        it('saves a changed project description and goes back to the project', async () => {
            signIn('user2') // a member, not the owner
            const recorder = installApiMock([...baseWorld(), echoPut(projectPut, FIXTURE_PROJECT)])
            const {user} = renderApp({route: `/projects/${name}`})

            await user.click(await screen.findByRole('button', {name: 'Edit'}))
            const field = await screen.findByLabelText('Description')
            expect(field).toHaveValue(projectDescription)
            // Nothing to save until something changed.
            expect(screen.getByRole('button', {name: 'Save Description'})).toBeDisabled()

            await user.clear(field)
            await user.type(field, '  Holography of Ni nanowires  ')
            await user.click(screen.getByRole('button', {name: 'Save Description'}))

            expect(await screen.findByText('Description saved.')).toBeInTheDocument()
            expect(await screen.findByRole('heading', {level: 1, name})).toBeInTheDocument()
            const puts = recorder.matching('PUT', projectPut)
            expect(puts).toHaveLength(1)
            expect(puts[0].body).toEqual({description: 'Holography of Ni nanowires'})
        })

        it('removes the description when the text is emptied', async () => {
            signIn('user1')
            const recorder = installApiMock([...baseWorld(), echoPut(projectPut, FIXTURE_PROJECT)])
            const {user} = renderApp({route: `/projects/${name}/description`})

            await user.clear(await screen.findByLabelText('Description'))
            await user.click(screen.getByRole('button', {name: 'Save Description'}))

            expect(await screen.findByText('Description removed.')).toBeInTheDocument()
            expect(recorder.matching('PUT', projectPut)[0].body).toEqual({description: ''})
        })

        it("shows the API's reason and keeps the text when the change is refused", async () => {
            signIn('user1')
            installApiMock([
                ...baseWorld(),
                {
                    method: 'PUT',
                    path: projectPut,
                    respond: () => ({
                        status: 400,
                        body: {
                            code: 'description_rejected',
                            message: 'README.md is not a regular file, so it is left alone.',
                        },
                    }),
                },
            ])
            const {user} = renderApp({route: `/projects/${name}/description`})

            const field = await screen.findByLabelText('Description')
            await user.type(field, ' more')
            await user.click(screen.getByRole('button', {name: 'Save Description'}))

            expect(await screen.findByText('README.md is not a regular file, so it is left alone.'))
                .toBeInTheDocument()
            expect(screen.getByLabelText('Description')).toHaveValue(`${projectDescription} more`)
        })

        it('turns away someone outside the project', async () => {
            signIn(outsider)
            installApiMock(baseWorld())
            renderApp({route: `/projects/${name}/description`})

            expect(await screen.findByText('Only the owner and the members can change descriptions'))
                .toBeInTheDocument()
            expect(screen.queryByLabelText('Description')).not.toBeInTheDocument()
        })
    })

    describe('sessions', () => {
        it('shows a session\'s first line in its row, with the whole text on hover', async () => {
            signIn(FIXTURE_PROJECT.owner)
            installApiMock(baseWorld())
            renderApp({route: `/projects/${name}`})
            await screen.findByRole('heading', {level: 1, name})

            const digest = within(rowOf(session.name)).getByText('First light')
            expect(digest).toHaveAttribute('title', session.description)
        })

        it('finds a session by words of its description', async () => {
            signIn(FIXTURE_PROJECT.owner)
            installApiMock(baseWorld())
            const {user} = renderApp({route: `/projects/${name}`})
            await screen.findByRole('heading', {level: 1, name})

            await user.type(screen.getByPlaceholderText('Search sessions…'), 'service visit')

            const rows = within(screen.getByRole('table')).getAllByRole('row').slice(1)
            expect(rows).toHaveLength(1)
            expect(within(rows[0]).getByText(session.name)).toBeInTheDocument()
        })

        it("edits a session's description from its row", async () => {
            signIn('user2')
            const recorder = installApiMock([...baseWorld(), echoPut(sessionPut, session)])
            const {user} = renderApp({route: `/projects/${name}`})

            await user.click(await screen.findByRole('link', {name: `Edit the description of ${session.name}`}))
            expect(await screen.findByRole('heading', {name: 'Session description'})).toBeInTheDocument()
            const field = screen.getByLabelText('Description')
            expect(field).toHaveValue(session.description)

            await user.clear(field)
            await user.type(field, 'Alignment only')
            await user.click(screen.getByRole('button', {name: 'Save Description'}))

            expect(await screen.findByText('Description saved.')).toBeInTheDocument()
            // The session name travels URL-encoded in the path.
            expect(recorder.matching('PUT', sessionPut)[0].body).toEqual({description: 'Alignment only'})
        })

        it('says so for a session that is not there', async () => {
            signIn('user1')
            installApiMock(baseWorld())
            renderApp({route: `/projects/${name}/sessions/nope/description`})

            expect(await screen.findByText('No such session')).toBeInTheDocument()
        })
    })

    describe('creating', () => {
        it('sends a new project\'s description, trimmed', async () => {
            signIn('user1')
            const created = {...FIXTURE_PROJECT, name: 'ghost_imaging_2026', members: ['user1']}
            const recorder = installApiMock([
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
                {method: 'POST', path: '/mgmt-api/projects', respond: () => ({status: 201, body: created})},
            ])
            const {user} = renderApp({route: '/projects/new'})

            await user.type(await screen.findByLabelText(/project name/i), created.name)
            await user.type(screen.getByLabelText(/description/i), '  Ni nanowires  ')
            await user.click(screen.getByRole('button', {name: /create project/i}))

            expect(await screen.findByText(`Project "${created.name}" created.`)).toBeInTheDocument()
            expect(recorder.matching('POST', '/mgmt-api/projects')[0].body)
                .toEqual({name: created.name, members: [], description: 'Ni nanowires'})
        })

        it('sends a new session\'s description and leaves out a blank one', async () => {
            signIn('user1')
            const createSession = `/mgmt-api/projects/${name}/sessions`
            const recorder = installApiMock([
                ...baseWorld(),
                {
                    method: 'POST',
                    path: createSession,
                    respond: ({body}) => ({
                        status: 201,
                        body: {...(body as object), immutable: false, ctime: 1, mtime: 1},
                    }),
                },
            ])
            const {user} = renderApp({route: `/projects/${name}/sessions/new`})

            const input = await screen.findByLabelText('Session name')
            await user.clear(input)
            await user.type(input, 'run 1')
            await user.type(screen.getByLabelText(/description/i), 'Titan, 300 kV')
            await user.click(screen.getByRole('button', {name: /create session/i}))

            expect(await screen.findByText('Session "run 1" created.')).toBeInTheDocument()
            expect(recorder.matching('POST', createSession)[0].body)
                .toEqual({name: 'run 1', description: 'Titan, 300 kV'})
        })

        it('refuses a session named README.md before asking the API', async () => {
            signIn('user1')
            const recorder = installApiMock(baseWorld())
            const {user} = renderApp({route: `/projects/${name}/sessions/new`})

            const input = await screen.findByLabelText('Session name')
            await user.clear(input)
            await user.type(input, 'ReadMe.md')
            await user.click(screen.getByRole('button', {name: /create session/i}))

            expect(await screen.findByText(/cannot be named README\.md/)).toBeInTheDocument()
            expect(recorder.matching('POST', `/mgmt-api/projects/${name}/sessions`)).toHaveLength(0)
        })
    })
})
