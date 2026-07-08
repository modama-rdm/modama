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

/** `PATCH /api/project/<name>/members/` success - the updated project. */
export interface ChangeMembersResponse extends OkEnvelope {
    project: SerializedProject
}

export interface CreateShareRequest {
    project: string
    dataset: string
    resource?: string
}

/**
 * `POST /api/share/` success — the freshly created share. The endpoint unwraps
 * the smbdrop `{share}` envelope and returns the bare {@link SerializedShare}.
 * The `smb_host`/`workgroup` needed to render connection details are not part of
 * this payload; the client supplies them (see `lib/shares.ts`).
 */
export type CreateShareResponse = SerializedShare

/** `DELETE /api/share/<share_id>/` success. */
export interface DeleteShareResponse {
    status: 'ok'
}

/**
 * Server-computed access links / transfer commands for a browse resource. Keys
 * are optional because which ones apply depends on the resource: the current
 * item carries the app links + rsync/sftp/SMB; listing rows carry only the
 * columns `browse.html.j2` shows (Nextcloud/JupyterHub for dirs, image viewer
 * for files, LiberTEM + SampleDB for both). The `..` row carries none.
 */
export interface BrowseAccess {
    nextcloud?: string
    jupyter?: string
    libertem?: string
    imageviewer?: string
    sampledb?: string
    sampledb_erc3?: string
    elabftw_erc3?: string
    rsync?: string
    sftp?: string
    smb_windows?: string
    smb_unix?: string
}

export interface BrowseInfo {
    name: string
    isdir: boolean
    /** Owning project name, or null outside a project tree. */
    project: string | null
    /** WebDAV timestamps, passed through as-is (may be null). */
    created: string | null
    modified: string | null
    /** WebDAV reports size as a string; null for directories. */
    size: string | number | null
    member_count: number
    /** True when the listing was capped at 1024 entries. */
    truncated: boolean
}

export interface BrowseEntry {
    name: string
    /** Normalized path; null only would be unexpected. `..` points at the parent. */
    path: string
    isdir: boolean
    created: string | null
    modified: string | null
    size: string | number | null
    access: BrowseAccess
}

/** `GET /api/browse/?path=` success. */
export interface BrowseSuccess extends OkEnvelope {
    /** Canonical (normalized) path — the client syncs its URL to this. */
    path: string
    info: BrowseInfo
    access: BrowseAccess
    /** Breadcrumb: `[fullPath, tail]` from the root down to the current path. */
    path_pyramid: [string, string][]
    members: BrowseEntry[]
}

/** `GET /api/browse/?path=` logical error (e.g. path not found), errno != 0. */
export interface BrowseErrorResponse extends GenericErrorResponse {
    path: string
    /** Nearest existing ancestor to offer as a way back. */
    backto: string | null
}

export type BrowseResponse = BrowseSuccess | BrowseErrorResponse
