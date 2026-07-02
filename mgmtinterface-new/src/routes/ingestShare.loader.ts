import {redirect, type LoaderFunctionArgs} from 'react-router-dom'
import {getCurrentUser} from '../api/auth'
import {Unauthorized, apiFetch} from '../api/client'
import type {ListSharesResponse, ProjectDetailsResponse} from '../api/types'

export interface IngestShareLoaderData {
    name: string
    dataset: string
    currentUser: string
    /** The current user's active ingest shares, keyed by share_id. */
    shares: ListSharesResponse
    /**
     * Project + datasets. Deferred (not awaited) because the backend computes
     * `usage_bytes` eagerly and that is slow — the screen streams it in behind a
     * skeleton via `<Await>`, matching the Project Details / Manage Members
     * loaders. Needed for `relpath` (to match the share), `members`/`owner` (the
     * manage-permission guard) and the dataset's sealed state.
     */
    details: Promise<ProjectDetailsResponse>
}

/**
 * Loads the user's shares (fast, needed to know the current share state) and
 * streams the project details for the session's ingest screen.
 */
export async function ingestShareLoader({
                                            params,
                                        }: LoaderFunctionArgs): Promise<IngestShareLoaderData> {
    const name = params.name ?? ''
    const dataset = params.dataset ?? ''
    const currentUser = getCurrentUser() ?? ''
    const next = `/projects/${encodeURIComponent(name)}/sessions/${encodeURIComponent(dataset)}/ingest`
    try {
        const shares = await apiFetch<ListSharesResponse>('/api/share/')
        const details = apiFetch<ProjectDetailsResponse>(
            `/api/project/${encodeURIComponent(name)}/`,
        )
        return {name, dataset, currentUser, shares, details}
    } catch (err) {
        if (err instanceof Unauthorized) throw redirect(`/login?next=${next}`)
        throw err
    }
}
