import {redirect, type LoaderFunctionArgs} from 'react-router-dom'
import {getCurrentUser} from '../api/auth'
import {Unauthorized, apiFetch} from '../api/client'
import type {GetUserListResponse, ProjectDetailsResponse} from '../api/types'

export interface ManageMembersLoaderData {
    name: string
    currentUser: string
    /** Every known user, for the member picker. */
    users: string[]
    /**
     * Project + datasets. Deferred (not awaited) because the backend computes
     * `usage_bytes` eagerly and that is slow - the form streams in behind a
     * skeleton via `<Await>` once the current membership is known.
     */
    details: Promise<ProjectDetailsResponse>
}

/** Loads the user directory (fast) and streams the project details for editing. */
export async function manageMembersLoader({
                                              params,
                                          }: LoaderFunctionArgs): Promise<ManageMembersLoaderData> {
    const name = params.name ?? ''
    const currentUser = getCurrentUser() ?? ''
    const next = `/projects/${encodeURIComponent(name)}/members`
    try {
        const res = await apiFetch<GetUserListResponse>('/api/users/list/')
        const details = apiFetch<ProjectDetailsResponse>(
            `/api/project/${encodeURIComponent(name)}/`,
        )
        return {name, currentUser, users: res.users, details}
    } catch (err) {
        if (err instanceof Unauthorized) throw redirect(`/login?next=${next}`)
        throw err
    }
}
