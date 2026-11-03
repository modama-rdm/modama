import {screen, within} from '@testing-library/react'
import {describe, expect, it} from 'vitest'
import type {BrowseEntry, BrowseV2} from '../../api/types'
import {installApiMock, type ApiHandler} from '../api-mock'
import {renderApp, signIn} from '../render'
import {baseWorld} from '../world'
import {FIXTURE_BROWSE} from '../fixtures/browse'
import {FIXTURE_DATASETS, FIXTURE_PROJECT} from '../fixtures/project'

/**
 * Descriptions, kept as README.md in the project or session folder. The lists
 * show the first line with the whole text on hover, the project page renders
 * the project's as Markdown, the search covers them, and the owner and the
 * members change them on their own screen. The texts are the hand-written
 * ones in the fixtures (see `fixtures/README.md`).
 *
 * Who may change one is the file system's call: the API writes README.md as
 * the signed-in user, so only whoever wrote it first may change it.
 *
 * A session's whole description opens in a dialog from its row. The data
 * browser renders a project's or a session's above the files in its folder.
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

        it.each([
            {
                why: 'README.md is not a plain file',
                status: 400,
                code: 'description_rejected',
                message: `README.md in project '${name}' is not a plain file (it is a folder or a FIFO), so it is left alone.`,
            },
            {
                // user2 is a member, but README.md is someone else's file.
                why: 'someone else wrote README.md',
                status: 403,
                code: 'no_access',
                message: `Only the person who created README.md in project '${name}' can change it.`,
            },
        ])("shows the API's reason and keeps the text when $why", async ({status, code, message}) => {
            signIn('user2')
            installApiMock([
                ...baseWorld(),
                {method: 'PUT', path: projectPut, respond: () => ({status, body: {code, message}})},
            ])
            const {user} = renderApp({route: `/projects/${name}/description`})

            const field = await screen.findByLabelText('Description')
            await user.type(field, ' more')
            await user.click(screen.getByRole('button', {name: 'Save Description'}))

            expect(await screen.findByText(message)).toBeInTheDocument()
            expect(screen.getByLabelText('Description')).toHaveValue(`${projectDescription} more`)
        })

        it('says who may change the file', async () => {
            signIn('user2')
            installApiMock(baseWorld())
            renderApp({route: `/projects/${name}/description`})

            expect(await screen.findByText(/belongs to whoever writes it first/)).toBeInTheDocument()
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

        it("shows a session's whole description in a dialog, and edits it from there", async () => {
            signIn('user2')
            const recorder = installApiMock([...baseWorld(), echoPut(sessionPut, session)])
            const {user} = renderApp({route: `/projects/${name}`})

            await user.click(await screen.findByRole('button', {name: `Show the description of ${session.name}`}))
            const dialog = screen.getByRole('dialog', {name: session.name})
            // Rendered, `#` shifted down as on the project page.
            expect(await within(dialog).findByRole('heading', {level: 3, name: 'First light'})).toBeInTheDocument()
            expect(within(dialog).getByText('Alignment of the new detector after the service visit.'))
                .toBeInTheDocument()

            await user.click(within(dialog).getByRole('button', {name: 'Edit'}))
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

        it('closes the dialog, which has no Edit for someone outside the project', async () => {
            signIn(outsider)
            installApiMock(baseWorld())
            const {user} = renderApp({route: `/projects/${name}`})

            await user.click(await screen.findByRole('button', {name: `Show the description of ${session.name}`}))
            const dialog = screen.getByRole('dialog', {name: session.name})
            expect(within(dialog).queryByRole('button', {name: 'Edit'})).not.toBeInTheDocument()

            await user.click(within(dialog).getByRole('button', {name: 'Close'}))
            expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
        })

        it('closes the dialog on a click on the backdrop, but not after selecting text in it', async () => {
            signIn('user2')
            installApiMock(baseWorld())
            const {user} = renderApp({route: `/projects/${name}`})

            await user.click(await screen.findByRole('button', {name: `Show the description of ${session.name}`}))
            const dialog = screen.getByRole('dialog', {name: session.name})
            const text = await within(dialog).findByText('Alignment of the new detector after the service visit.')

            // Pressed in the text, let go over the backdrop: a selection.
            await user.pointer([
                {keys: '[MouseLeft>]', target: text},
                {target: dialog},
                {keys: '[/MouseLeft]', target: dialog},
            ])
            expect(screen.getByRole('dialog', {name: session.name})).toBeInTheDocument()

            // The dialog element itself is the backdrop: the panel fills it.
            await user.click(dialog)
            expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
        })

        it('offers a member to add one under the name, with nothing next to the name itself', async () => {
            signIn('user2')
            installApiMock(baseWorld())
            const {user} = renderApp({route: `/projects/${name}`})
            await screen.findByRole('heading', {level: 1, name})

            // Nothing beside a name that could read as "rename this session".
            for (const row of within(screen.getByRole('table')).getAllByRole('row').slice(1)) {
                const nameLine = within(row).getAllByRole('cell')[0].firstElementChild as HTMLElement
                expect(within(nameLine).queryByRole('link')).toBeNull()
                expect(within(nameLine).queryByRole('button')).toBeNull()
            }

            await user.click(screen.getByRole('link', {name: 'Add a description to demo session'}))

            expect(await screen.findByRole('heading', {name: 'Session description'})).toBeInTheDocument()
            expect(screen.getByLabelText('Description')).toHaveValue('')
        })

        it('offers no way to add one to someone outside the project', async () => {
            signIn(outsider)
            installApiMock(baseWorld())
            renderApp({route: `/projects/${name}`})
            await screen.findByRole('heading', {level: 1, name})

            expect(screen.queryByRole('link', {name: /add a description to/i})).not.toBeInTheDocument()
        })

        it('says so for a session that is not there', async () => {
            signIn('user1')
            installApiMock(baseWorld())
            renderApp({route: `/projects/${name}/sessions/nope/description`})

            expect(await screen.findByText('No such session')).toBeInTheDocument()
        })
    })

    describe('data browser', () => {
        const projectFolder = `/${FIXTURE_PROJECT.relpath}`
        const sessionFolder = `${projectFolder}/${session.name}`
        const entry = (folder: string, entryName: string, isdir: boolean): BrowseEntry => ({
            name: entryName,
            path: `${folder}/${entryName}`,
            isdir,
            mtime: 1,
            size: isdir ? null : 64,
            access: {},
        })
        const listingOf = (folder: string, entries: BrowseEntry[]): BrowseV2 => ({
            ...FIXTURE_BROWSE,
            path: folder,
            name: folder.split('/').pop()!,
            project: name,
            entries,
            total: entries.length,
            next_cursor: null,
        })
        const listing = (folder: string, entries: BrowseEntry[]): ApiHandler => ({
            method: 'GET',
            path: '/mgmt-api/browse',
            respond: () => ({body: listingOf(folder, entries)}),
        })
        const browse = (folder: string) => `/browse?path=${encodeURIComponent(folder)}`

        it("renders a session folder's README.md above the listing, with Edit for a member", async () => {
            signIn('user2')
            installApiMock([
                listing(sessionFolder, [entry(sessionFolder, 'raw', true), entry(sessionFolder, 'README.md', false)]),
                ...baseWorld(),
            ])
            const {user} = renderApp({route: browse(sessionFolder)})

            expect(await screen.findByRole('heading', {level: 3, name: 'First light'})).toBeInTheDocument()
            expect(screen.getByText('Alignment of the new detector after the service visit.')).toBeInTheDocument()

            await user.click(screen.getByRole('button', {name: 'Edit'}))
            expect(await screen.findByRole('heading', {name: 'Session description'})).toBeInTheDocument()
        })

        it("renders a project folder's README.md, without Edit for someone outside the project", async () => {
            signIn(outsider)
            installApiMock([
                listing(projectFolder, [entry(projectFolder, session.name, true), entry(projectFolder, 'README.md', false)]),
                ...baseWorld(),
            ])
            renderApp({route: browse(projectFolder)})

            expect(await screen.findByRole('heading', {level: 3, name: 'Data management dogfooding'}))
                .toBeInTheDocument()
            expect(screen.queryByRole('button', {name: 'Edit'})).not.toBeInTheDocument()
        })

        it('shows one README.md at a time while moving between folders that have one', async () => {
            signIn('user2')
            const folders: Record<string, BrowseEntry[]> = {
                [projectFolder]: [entry(projectFolder, session.name, true), entry(projectFolder, 'README.md', false)],
                [sessionFolder]: [entry(sessionFolder, 'raw', true), entry(sessionFolder, 'README.md', false)],
            }
            installApiMock([
                {
                    method: 'GET',
                    path: '/mgmt-api/browse',
                    respond: ({params}) => ({body: listingOf(params.get('path')!, folders[params.get('path')!])}),
                },
                ...baseWorld(),
            ])
            const {user} = renderApp({route: browse(projectFolder)})
            await screen.findByRole('heading', {level: 3, name: 'Data management dogfooding'})

            await user.click(screen.getByRole('link', {name: session.name}))
            expect(await screen.findByRole('heading', {level: 3, name: 'First light'})).toBeInTheDocument()
            expect(screen.getAllByRole('heading', {name: 'README.md'})).toHaveLength(1)
            expect(screen.queryByRole('heading', {name: 'Data management dogfooding'})).not.toBeInTheDocument()

            await user.click(screen.getByRole('link', {name: '..'}))
            expect(await screen.findByRole('heading', {level: 3, name: 'Data management dogfooding'}))
                .toBeInTheDocument()
            expect(screen.getAllByRole('heading', {name: 'README.md'})).toHaveLength(1)
            expect(screen.queryByRole('heading', {name: 'First light'})).not.toBeInTheDocument()
        })

        it('looks nothing up for a folder without README.md', async () => {
            signIn('user2')
            const recorder = installApiMock([listing(sessionFolder, [entry(sessionFolder, 'raw', true)]), ...baseWorld()])
            renderApp({route: browse(sessionFolder)})

            expect(await screen.findByText('raw')).toBeInTheDocument()
            expect(recorder.matching('GET', `/mgmt-api/projects/${name}`)).toHaveLength(0)
            expect(screen.queryByText('README.md')).not.toBeInTheDocument()
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

        it('goes on to the description, text kept, when the project was made but README.md was not', async () => {
            signIn('user1')
            const created = {...FIXTURE_PROJECT, name: 'ghost_imaging_2026', members: ['user1'], description: null}
            const createdPut = `/mgmt-api/projects/${created.name}/description`
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
                {
                    method: 'POST',
                    path: '/mgmt-api/projects',
                    respond: () => ({
                        status: 502,
                        body: {
                            code: 'description_not_saved',
                            message: `The project '${created.name}' was created, but its description could not be saved. Please add it again on the project page.`,
                        },
                    }),
                },
                echoPut(createdPut, created),
            ])
            const {user} = renderApp({route: '/projects/new'})

            await user.type(await screen.findByLabelText(/project name/i), created.name)
            await user.type(screen.getByLabelText(/description/i), 'Ni nanowires')
            await user.click(screen.getByRole('button', {name: /create project/i}))

            // Not the create form again, which would now answer "already exists".
            expect(await screen.findByRole('heading', {name: 'Project description'})).toBeInTheDocument()
            expect(screen.getByText(
                `Project "${created.name}" was created, but its description could not be saved. Please save it again.`,
            )).toBeInTheDocument()
            expect(screen.getByLabelText('Description')).toHaveValue('Ni nanowires')

            await user.click(screen.getByRole('button', {name: 'Save Description'}))

            expect(await screen.findByText('Description saved.')).toBeInTheDocument()
            expect(recorder.matching('PUT', createdPut)[0].body).toEqual({description: 'Ni nanowires'})
        })

        it('does the same for a session', async () => {
            signIn('user1')
            const created = {name: 'run 1', immutable: false, ctime: 1, mtime: 1, description: null}
            installApiMock([
                {
                    method: 'GET',
                    path: `/mgmt-api/projects/${name}`,
                    respond: () => ({
                        body: {...FIXTURE_PROJECT, usage_bytes: null, sessions: [...FIXTURE_DATASETS, created]},
                    }),
                },
                ...baseWorld(),
                {
                    method: 'POST',
                    path: `/mgmt-api/projects/${name}/sessions`,
                    respond: () => ({
                        status: 502,
                        body: {
                            code: 'description_not_saved',
                            message: "The session 'run 1' was created, but its description could not be saved. Please add it again on the project page.",
                        },
                    }),
                },
            ])
            const {user} = renderApp({route: `/projects/${name}/sessions/new`})

            const input = await screen.findByLabelText('Session name')
            await user.clear(input)
            await user.type(input, 'run 1')
            await user.type(screen.getByLabelText(/description/i), 'Titan, 300 kV')
            await user.click(screen.getByRole('button', {name: /create session/i}))

            expect(await screen.findByRole('heading', {name: 'Session description'})).toBeInTheDocument()
            expect(screen.getByText(/Session "run 1" was created, but its description could not be saved/))
                .toBeInTheDocument()
            expect(screen.getByLabelText('Description')).toHaveValue('Titan, 300 kV')
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
