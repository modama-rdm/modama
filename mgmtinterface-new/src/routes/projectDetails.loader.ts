import {type LoaderFunctionArgs} from 'react-router-dom'
import {getCurrentUser} from '../api/auth'
import {apiFetch} from '../api/client'
import type {ListSharesResponse, ProjectDetailsResponse} from '../api/types'

export interface ProjectDetailsLoaderData {
    name: string
    currentUser: string
    /** Active ingest shares scoped to the current user (best-effort). */
    shares: ListSharesResponse
    /**
     * Project + datasets. Deferred (not awaited) because the backend computes
     * `usage_bytes` eagerly and that is slow — the screen streams it in behind a
     * skeleton via `<Await>` rather than blocking the navigation.
     */
    details: Promise<ProjectDetailsResponse>
}

/** Loads a single project's details (slow, streamed) + the user's shares. */
export async function projectDetailsLoader({
                                               params,
                                           }: LoaderFunctionArgs): Promise<ProjectDetailsLoaderData> {
    const name = params.name ?? ''
    const currentUser = getCurrentUser() ?? ''

    // Secondary to the page — never fail the whole screen if shares are down.
    let shares: ListSharesResponse
    try {
        shares = await apiFetch<ListSharesResponse>('/api/share/')
    } catch {
        shares = {}
    }

    const details = apiFetch<ProjectDetailsResponse>(
        `/api/project/${encodeURIComponent(name)}/`,
    )

    return {name, currentUser, shares, details}
}
