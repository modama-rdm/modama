import {redirect} from 'react-router-dom'
import {getCurrentUser} from '../api/auth'
import {Unauthorized, apiFetch} from '../api/client'
import {apiFetchV2} from '../api/clientV2'
import type {ListSharesResponse, ProjectListV2} from '../api/types'

export interface ProjectsLoaderData {
    currentUser: string
    projects: ProjectListV2['projects']
    shares: ListSharesResponse
}

/**
 * Loads the project list (+ the current user's active ingested shares).
 *
 * The list now comes from the new API (#172), authenticated by the session
 * cookie; shares are still on the legacy Flask endpoint with its Basic token.
 * The two coexist screen by screen until every endpoint has moved over.
 */
export async function projectsLoader(): Promise<ProjectsLoaderData> {
    const currentUser = getCurrentUser() ?? ''

    let projects: ProjectListV2['projects']
    try {
        projects = (await apiFetchV2<ProjectListV2>('/projects')).projects
    } catch (err) {
        if (err instanceof Unauthorized) throw redirect('/login?next=/projects')
        throw err
    }

    // Active ingest is secondary — never fail the whole page if shares are down.
    let shares: ListSharesResponse
    try {
        shares = await apiFetch<ListSharesResponse>('/api/share/')
    } catch {
        shares = {}
    }

    return {currentUser, projects, shares}
}
