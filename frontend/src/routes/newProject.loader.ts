import {redirect} from 'react-router-dom'
import {getCurrentUser} from '../api/auth'
import {Unauthorized, apiFetch} from '../api/client'
import type {GetUserListResponse} from '../api/types'

export interface NewProjectLoaderData {
    currentUser: string
    /** Every known user, for the member picker. */
    users: string[]
}

/** Loads the user directory used to populate the member multi-select. */
export async function newProjectLoader(): Promise<NewProjectLoaderData> {
    const currentUser = getCurrentUser() ?? ''
    try {
        const res = await apiFetch<GetUserListResponse>('/api/users/list/')
        return {currentUser, users: res.users}
    } catch (err) {
        if (err instanceof Unauthorized) throw redirect('/login?next=/projects/new')
        throw err
    }
}
