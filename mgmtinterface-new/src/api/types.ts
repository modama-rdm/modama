/**
 * TypeScript mirrors of the JSON shapes the app reads.
 *
 * Every screen but the data browser reads the new `/mgmt-api` service (GitLab
 * #172), which reports failure with the status code and a `{code, message}`
 * body (see `clientV2.ts`); its shapes carry a `V2` suffix. The browse types at
 * the end mirror the legacy Flask `/api/browse/` stopgap, the one endpoint
 * still answering HTTP 200 with an `errno` envelope; step 5 of #172 replaces it.
 */

/** The legacy envelope: `errno` 0 is success. Only the browse stopgap uses it. */
export interface OkEnvelope {
    errno: 0
}

export interface GenericErrorResponse {
    errno: number
    message: string
}

/**
 * One project, as the permissions service serializes it. `usage_bytes` is null
 * in every answer but {@link ProjectUsageV2}: computing it means a `du` over
 * the whole project, which nothing else should wait for.
 */
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

/** One session (a "dataset" to the permissions service): a folder with `raw/`. */
export interface SerializedDataset {
    name: string
    immutable: boolean
    ctime: number
    mtime: number
}

/**
 * `GET /mgmt-api/projects`. An object rather than a bare array so pagination
 * fields can arrive without breaking callers.
 */
export interface ProjectListV2 {
    projects: SerializedProject[]
}

/**
 * `GET /mgmt-api/projects/<name>`: the project's own fields plus its sessions,
 * flat, not the legacy `{project, datasets}` envelope. Readable by anyone
 * signed in. Fast: the size is a separate call ({@link ProjectUsageV2}).
 */
export interface ProjectDetailsV2 extends SerializedProject {
    sessions: SerializedDataset[]
}

/**
 * `GET /mgmt-api/projects/<name>/usage`: the one slow call (`du` on the
 * server, up to a minute on a large project). Loaders stream it separately so
 * the rest of a screen never waits for it.
 */
export interface ProjectUsageV2 {
    usage_bytes: number
}

/** `GET /mgmt-api/users`: everyone a project can have as a member, sorted. */
export interface UserListV2 {
    users: string[]
}

/**
 * `POST /mgmt-api/projects`. No `owner`: the signed-in session is the owner.
 * Answers 201 with the new project, or 200 with the existing one when the
 * signed-in user already owns a project of that name (the create endpoints
 * are idempotent); 409 when someone else does.
 */
export interface CreateProjectRequestV2 {
    name: string
    members: string[]
}

/**
 * `PUT /mgmt-api/projects/<name>/members`: the complete member list wanted.
 * The server works out who to add and who to remove, so sending the same list
 * again (after a lost or partial answer) is safe. The owner is a member
 * regardless and may be left out. Answers the {@link SerializedProject}.
 */
export interface SetMembersRequestV2 {
    members: string[]
}

/** `POST /mgmt-api/projects/<name>/sessions` — the session, by its UI name. */
export interface CreateSessionRequestV2 {
    name: string
}

/**
 * A session as the new API answers it: on create (200 when it already existed,
 * 201 when just created) and on `POST .../sessions/<session>/seal`, which
 * takes no body and answers the sealed session.
 */
export type SessionV2 = SerializedDataset

/** An smbdrop share, the fields the share service itself reports. */
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

/** `POST /mgmt-api/ingest-shares` — the session to open a share for. */
export interface CreateIngestShareRequestV2 {
    project: string
    session: string
}

/**
 * An ingest share as the new API answers it, on create and in the list: the
 * smbdrop fields plus what the legacy API left to the client — which project
 * and session it is for, and the host and workgroup to connect with. On create,
 * 200 means the user already had a share open for that session, 201 that a new
 * one was opened.
 */
export interface IngestShareV2 extends SerializedShare {
    project: string
    session: string
    smb_host: string
    workgroup: string
}

/**
 * `GET /mgmt-api/ingest-shares`: the signed-in user's open shares, oldest
 * first. A list rather than the legacy map keyed by share id; each share names
 * its project and session, so nothing parses `dest_path` any more.
 * `DELETE /mgmt-api/ingest-shares/<share_id>` answers 204, or 404 once the
 * share is gone.
 */
export interface IngestShareListV2 {
    shares: IngestShareV2[]
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
