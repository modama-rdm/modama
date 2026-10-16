import {redirect, type LoaderFunctionArgs} from 'react-router-dom'
import {getCurrentUser} from '../api/auth'
import {Unauthorized} from '../api/client'
import {apiFetchV2} from '../api/clientV2'
import type {IngestShareListV2, IngestShareV2, ProjectDetailsV2} from '../api/types'

export interface IngestShareLoaderData {
    name: string
    dataset: string
    currentUser: string
    /** The current user's open ingest shares; each names its project and session. */
    shares: IngestShareV2[]
    /**
     * Project + sessions. Not awaited, so the breadcrumbs paint at once and the
     * panel streams in behind a skeleton via `<Await>`, matching the Project
     * Details / Manage Members loaders. Needed for `members`/`owner` (the
     * manage-permission guard) and the session's sealed state.
     */
    details: Promise<ProjectDetailsV2>
}

/**
 * Loads the user's shares (needed to know the current share state) and streams
 * the project details for the session's ingest screen.
 */
export async function ingestShareLoader({
                                            params,
                                        }: LoaderFunctionArgs): Promise<IngestShareLoaderData> {
    const name = params.name ?? ''
    const dataset = params.dataset ?? ''
    const currentUser = getCurrentUser() ?? ''
    const next = `/projects/${encodeURIComponent(name)}/sessions/${encodeURIComponent(dataset)}/ingest`
    try {
        const shares = (await apiFetchV2<IngestShareListV2>('/ingest-shares')).shares
        const details = apiFetchV2<ProjectDetailsV2>(`/projects/${encodeURIComponent(name)}`)
        return {name, dataset, currentUser, shares, details}
    } catch (err) {
        if (err instanceof Unauthorized) throw redirect(`/login?next=${next}`)
        throw err
    }
}
