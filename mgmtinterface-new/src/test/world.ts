import {paramFrom, type ApiHandler, type ApiReply} from './api-mock'
import {FIXTURE_PROJECTS} from './fixtures/projects'
import {FIXTURE_PROJECT, FIXTURE_DATASETS} from './fixtures/project'
import {FIXTURE_USERS} from './fixtures/users'
import {FIXTURE_BROWSE} from './fixtures/browse'
import type {BrowseV2, IngestShareV2} from '../api/types'

export interface WorldOptions {
    /** The signed-in user's open ingest shares. Defaults to none. */
    shares?: IngestShareV2[]
}

const PROJECT = '/mgmt-api/projects/:name'
const USAGE = '/mgmt-api/projects/:name/usage'

/**
 * The read-only GETs every journey needs, built from the scrubbed prod fixtures
 * (see `test/fixtures/README.md`). A journey composes this with its own
 * mutation handlers (POST/PUT/DELETE) via `installApiMock([...baseWorld(), ...])`.
 *
 * `GET /mgmt-api/projects/:name` returns the fully-captured `FIXTURE_PROJECT`
 * (with its sessions) for its real name, and a thin (no sessions) answer built
 * from the matching `FIXTURE_PROJECTS` list entry for any other name — real
 * projects that were only ever captured at list-shape, not queried
 * individually. The size is its own, slow call on the new API, so the details
 * carry `usage_bytes: null` and `GET .../usage` answers the captured value.
 *
 * Everything is on the new API (#172): no `errno` envelope, failures are
 * status codes with a `{code, message}` body.
 */
export function baseWorld({shares = []}: WorldOptions = {}): ApiHandler[] {
    const notFound = (name: string) => ({
        status: 404,
        body: {code: 'project_not_found', message: `There is no project named '${name}'.`},
    })
    return [
        {
            method: 'GET',
            path: '/mgmt-api/projects',
            respond: () => ({body: {projects: FIXTURE_PROJECTS}}),
        },
        {
            method: 'GET',
            path: PROJECT,
            respond: ({path}) => {
                const name = paramFrom(PROJECT, path, 'name')
                if (name === FIXTURE_PROJECT.name) {
                    return {body: {...FIXTURE_PROJECT, usage_bytes: null, sessions: FIXTURE_DATASETS}}
                }
                const project = FIXTURE_PROJECTS.find(p => p.name === name)
                if (!project) return notFound(name)
                return {body: {...project, usage_bytes: null, sessions: []}}
            },
        },
        {
            method: 'GET',
            path: USAGE,
            respond: ({path}) => {
                const name = paramFrom(USAGE, path, 'name')
                if (name === FIXTURE_PROJECT.name) {
                    return {body: {usage_bytes: FIXTURE_PROJECT.usage_bytes ?? 0}}
                }
                if (!FIXTURE_PROJECTS.some(p => p.name === name)) return notFound(name)
                return {body: {usage_bytes: 0}}
            },
        },
        {
            method: 'GET',
            path: '/mgmt-api/users',
            respond: () => ({body: {users: FIXTURE_USERS}}),
        },
        {
            method: 'GET',
            path: '/mgmt-api/ingest-shares',
            respond: () => ({body: {shares}}),
        },
        {
            method: 'GET',
            path: '/mgmt-api/browse',
            respond: ({params}) => browseReply(params),
        },
    ]
}

/**
 * `GET /mgmt-api/browse` from the one captured listing: its own path answers
 * a page of it, anything else is a 404 with the API's error body. `pageSize`
 * overrides the `limit` the app asked for, so a journey can page a fixture
 * that would otherwise fit on one page.
 */
export function browseReply(params: URLSearchParams, pageSize?: number): ApiReply {
    // The API normalizes the path and answers the one spelling; a trailing
    // slash is the case the loader's redirect is tested with.
    const path = (params.get('path') ?? '/').replace(/\/+/g, '/').replace(/(.)\/$/, '$1')
    if (path !== FIXTURE_BROWSE.path) {
        return {
            status: 404,
            body: {code: 'path_not_found', message: `There is no file or folder at '${path}'.`},
        }
    }
    return {body: browsePage(FIXTURE_BROWSE, params, pageSize)}
}

/**
 * One page of `full`, the way the API pages: `cursor` names the last entry of
 * the page before (here simply its base64 name — the real cursor is opaque
 * too), and `next_cursor` is null on the last page.
 */
export function browsePage(full: BrowseV2, params: URLSearchParams, pageSize?: number): BrowseV2 {
    const limit = pageSize ?? Number(params.get('limit') ?? 200)
    const cursor = params.get('cursor')
    const start = cursor ? full.entries.findIndex(e => e.name === atob(cursor)) + 1 : 0
    const entries = full.entries.slice(start, start + limit)
    const last = entries[entries.length - 1]
    const next = last && start + limit < full.entries.length ? btoa(last.name) : null
    return {...full, entries, total: full.entries.length, next_cursor: next}
}
