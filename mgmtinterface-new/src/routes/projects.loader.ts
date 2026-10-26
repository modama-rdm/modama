import {redirect} from 'react-router-dom'
import {getCurrentUser} from '../api/auth'
import {Unauthorized} from '../api/errors'
import {apiFetchV2} from '../api/clientV2'
import type {IngestShareListV2, IngestShareV2, ProjectListV2} from '../api/types'

export interface ProjectsLoaderData {
    currentUser: string
    projects: ProjectListV2['projects']
    /** The current user's open ingest shares, for the active-upload banner. */
    shares: IngestShareV2[]
}

/**
 * Loads the project list and the current user's open ingest shares, both from
 * the new API (#172), authenticated by the session cookie.
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
    let shares: IngestShareV2[]
    try {
        shares = (await apiFetchV2<IngestShareListV2>('/ingest-shares')).shares
    } catch {
        shares = []
    }

    return {currentUser, projects, shares}
}
