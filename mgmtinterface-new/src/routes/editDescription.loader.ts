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

/**
 * Router state from a create form whose project or session was made but whose
 * description was not (`502 description_not_saved`): the text the user typed,
 * to save again, and why it is shown.
 */
export interface UnsavedDescription {
    text: string
    error: string
}

/** Streams the project details for editing a project's or a session's description. */
export function editDescriptionLoader({params}: LoaderFunctionArgs): EditDescriptionLoaderData {
    const name = params.name ?? ''
    return {
        name,
        session: params.dataset ?? null,
        currentUser: getCurrentUser() ?? '',
        details: apiFetchV2<ProjectDetailsV2>(`/projects/${encodeURIComponent(name)}`),
    }
}
