import type {LoaderFunctionArgs} from 'react-router-dom'
import {getCurrentUser} from '../api/auth'
import {apiFetchV2} from '../api/clientV2'
import type {ProjectDetailsV2} from '../api/types'

export interface EditDescriptionLoaderData {
    name: string
    /** The session whose description is edited, or null for the project's own. */
    session: string | null
    currentUser: string
    /**
     * Project + sessions, for the current text and the membership check. Not
     * awaited, so the breadcrumbs paint at once; a 401 is handled where it is
     * awaited (`<Await errorElement>`), as on the members screen.
     */
    details: Promise<ProjectDetailsV2>
}

/** Streams the project details for editing a project's or a session's description (#112). */
export function editDescriptionLoader({params}: LoaderFunctionArgs): EditDescriptionLoaderData {
    const name = params.name ?? ''
    return {
        name,
        session: params.dataset ?? null,
        currentUser: getCurrentUser() ?? '',
        details: apiFetchV2<ProjectDetailsV2>(`/projects/${encodeURIComponent(name)}`),
    }
}
