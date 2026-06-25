import {redirect} from 'react-router-dom'
import {getCurrentUser} from '../api/auth'
import {Unauthorized, apiFetch} from '../api/client'
import type {ListProjectsResponse, ListSharesResponse} from '../api/types'

export interface ProjectsLoaderData {
    currentUser: string
    projects: ListProjectsResponse['projects']
    shares: ListSharesResponse
}

/** Loads the project list (+ the current user's active ingested shares). */
export async function projectsLoader(): Promise<ProjectsLoaderData> {
    const currentUser = getCurrentUser() ?? ''

    let projects: ListProjectsResponse['projects']
    try {
        projects = (await apiFetch<ListProjectsResponse>('/api/project/')).projects
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
