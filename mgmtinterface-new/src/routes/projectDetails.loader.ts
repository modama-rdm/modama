import {type LoaderFunctionArgs} from 'react-router-dom'
import {getCurrentUser} from '../api/auth'
import {apiFetchV2} from '../api/clientV2'
import type {
    IngestShareListV2,
    IngestShareV2,
    ProjectDetailsV2,
    ProjectUsageV2,
} from '../api/types'

export interface ProjectDetailsLoaderData {
    name: string
    currentUser: string
    /** The current user's open ingest shares, for the Sharing badges (best-effort). */
    shares: IngestShareV2[]
    /**
     * Project + sessions. Not awaited, so the breadcrumbs paint at once and the
     * body streams in behind a skeleton via `<Await>`. It resolves quickly: the
     * new API answers the details without computing the size.
     */
    details: Promise<ProjectDetailsV2>
    /**
     * Disk usage, the one slow call (`du` over the project on the server, up to
     * a minute on a large one). Streamed into the Size row on its own, so the
     * rest of the page never waits for it.
     */
    usage: Promise<ProjectUsageV2>
}

/** Loads a single project's details and size (both streamed) + the user's shares. */
export async function projectDetailsLoader({
                                               params,
                                           }: LoaderFunctionArgs): Promise<ProjectDetailsLoaderData> {
    const name = params.name ?? ''
    const currentUser = getCurrentUser() ?? ''

    // Secondary to the page — never fail the whole screen if shares are down.
    let shares: IngestShareV2[]
    try {
        shares = (await apiFetchV2<IngestShareListV2>('/ingest-shares')).shares
    } catch {
        shares = []
    }

    const path = `/projects/${encodeURIComponent(name)}`
    const details = apiFetchV2<ProjectDetailsV2>(path)
    const usage = apiFetchV2<ProjectUsageV2>(`${path}/usage`)

    return {name, currentUser, shares, details, usage}
}
