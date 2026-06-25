/**
 * TypeScript mirrors of the mgmtinterface `/api/*` JSON shapes.
 * */

/** Most endpoints return an `errno` discriminator (0 = success). */
export interface OkEnvelope {
    errno: 0
}

export interface GenericErrorResponse {
    errno: number
    message: string
}

export interface IdInner {
    uid: number
}

export interface AuthenticateSuccess extends OkEnvelope {
    id: IdInner
}

/** `POST /api/users/auth/` — public; returns 200 even for bad credentials. */
export type AuthenticateResponse = AuthenticateSuccess | GenericErrorResponse

export interface GetUserListResponse extends OkEnvelope {
    users: string[]
}

export interface SerializedProject {
    relpath: string
    name: string
    owner: string
    members: string[]
    ctime: number
    mtime: number
    gid: number
    usage_bytes: number | null
}

export interface SerializedDataset {
    name: string
    immutable: boolean
    ctime: number
    mtime: number
}

export interface ListProjectsResponse extends OkEnvelope {
    projects: SerializedProject[]
}

export interface ProjectDetailsResponse extends OkEnvelope {
    project: SerializedProject
    datasets: SerializedDataset[]
}

export interface SerializedShare {
    resource: string | null
    share_id: string
    share_name: string
    dest_path: string
    created_timestamp: number
    /** Temporary share credentials. */
    username: string
    password: string
    /** Impersonated Unix identity. */
    uid: number
    gid: number
}

/** `GET /api/share/` — keyed by share_id. */
export type ListSharesResponse = Record<string, SerializedShare>


export interface CreateProjectRequest {
    name: string
    owner: string
    members: string[]
}

/** `POST /api/project/` success — the freshly created project. */
export interface CreateProjectResponse extends OkEnvelope {
    project: SerializedProject
}

export interface CreateDatasetRequest {
    name: string
    owner: string
}

/** `POST /api/project/<name>/dataset/` — success carries no payload. */
export type CreateDatasetResponse = OkEnvelope

export interface SealDatasetRequest {
    name: string
    user: string
}

/** `POST /api/project/<name>/dataset/seal/` — success carries no payload. */
export type SealDatasetResponse = OkEnvelope

export interface ChangeMembersRequest {
    op: 'add' | 'remove'
    members: string[]
    user: string
}

export interface CreateShareRequest {
    project: string
    dataset: string
    resource?: string
}
