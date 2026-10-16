import {redirect, type LoaderFunctionArgs} from 'react-router-dom'
import {getCurrentUser} from '../api/auth'
import {Unauthorized} from '../api/client'
import {apiFetchV2} from '../api/clientV2'
import type {ProjectDetailsV2, UserListV2} from '../api/types'

export interface ManageMembersLoaderData {
    name: string
    currentUser: string
    /** Every known user, for the member picker. */
    users: string[]
    /**
     * Project + sessions. Not awaited, so the breadcrumbs paint at once and the
     * form streams in behind a skeleton via `<Await>` once the current
     * membership is known.
     */
    details: Promise<ProjectDetailsV2>
}

/** Loads the user directory and streams the project details for editing. */
export async function manageMembersLoader({
                                              params,
                                          }: LoaderFunctionArgs): Promise<ManageMembersLoaderData> {
    const name = params.name ?? ''
    const currentUser = getCurrentUser() ?? ''
    const next = `/projects/${encodeURIComponent(name)}/members`
    try {
        const res = await apiFetchV2<UserListV2>('/users')
        const details = apiFetchV2<ProjectDetailsV2>(`/projects/${encodeURIComponent(name)}`)
        return {name, currentUser, users: res.users, details}
    } catch (err) {
        if (err instanceof Unauthorized) throw redirect(`/login?next=${next}`)
        throw err
    }
}
