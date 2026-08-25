import {paramFrom, type ApiHandler} from './api-mock'
import {FIXTURE_PROJECTS} from './fixtures/projects'
import {FIXTURE_PROJECT, FIXTURE_DATASETS} from './fixtures/project'
import {FIXTURE_USERS} from './fixtures/users'
import {FIXTURE_BROWSE} from './fixtures/browse'
import type {ListSharesResponse} from '../api/types'

export interface WorldOptions {
    /** Active ingest shares, keyed by share_id. Defaults to none. */
    shares?: ListSharesResponse
}

/**
 * The read-only GETs every journey needs, built from the scrubbed prod fixtures
 * (see `test/fixtures/README.md`). A journey composes this with its own
 * mutation handlers (POST/PATCH/DELETE) via `installApiMock([...baseWorld(), ...])`.
 *
 * `GET /api/project/:name/` returns the fully-captured `FIXTURE_PROJECT` for its
 * real name, and a thin (empty-datasets) envelope built from the matching
 * `FIXTURE_PROJECTS` list entry for any other name — real projects that were
 * only ever captured at list-shape, not queried individually.
 *
 * Handlers here span both APIs while the migration runs: `/mgmt-api/*` answers
 * without an `errno` envelope, the legacy `/api/*` ones still carry it.
 */
export function baseWorld({shares = {}}: WorldOptions = {}): ApiHandler[] {
    return [
        {
            // Migrated to the new API (#172): no `errno`, failures would be
            // status codes. The project shape itself is unchanged, so the same
            // fixture serves both.
            method: 'GET',
            path: '/mgmt-api/projects',
            respond: () => ({body: {projects: FIXTURE_PROJECTS}}),
        },
        {
            method: 'GET',
            path: '/api/project/:name/',
            respond: ({path}) => {
                const name = paramFrom('/api/project/:name/', path, 'name')
                if (name === FIXTURE_PROJECT.name) {
                    return {body: {errno: 0, project: FIXTURE_PROJECT, datasets: FIXTURE_DATASETS}}
                }
                const project = FIXTURE_PROJECTS.find(p => p.name === name)
                if (!project) return {status: 404, body: {errno: 1, message: 'not found'}}
                return {body: {errno: 0, project, datasets: []}}
            },
        },
        {
            method: 'GET',
            path: '/api/users/list/',
            respond: () => ({body: {errno: 0, users: FIXTURE_USERS}}),
        },
        {
            method: 'GET',
            path: '/api/share/',
            respond: () => ({body: shares}),
        },
        {
            method: 'GET',
            path: '/api/browse/',
            respond: () => ({body: FIXTURE_BROWSE}),
        },
    ]
}
