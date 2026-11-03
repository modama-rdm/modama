/**
 * TypeScript mirrors of the JSON shapes the app reads.
 *
 * Every screen reads the new `/mgmt-api` service (GitLab #172), which reports
 * failure with the status code and a `{code, message}` body (see
 * `clientV2.ts`); its shapes carry a `V2` suffix.
 */

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
    /**
     * The text of `README.md` in the project folder, or null when there is
     * none. Markdown, as people write it. Absent from an API that predates
     * descriptions.
     */
    description?: string | null
}

/** One session (a "dataset" to the permissions service): a folder with `raw/`. */
export interface SerializedDataset {
    name: string
    immutable: boolean
    ctime: number
    mtime: number
    /** The text of `README.md` in the session folder, next to `raw/`, or null. */
    description?: string | null
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
    /**
     * Saved as `README.md` in the new folder. Left out when blank; the server
     * treats blank as absent anyway. Up to 10 000 characters.
     */
    description?: string
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
    /** Saved as `README.md` in the session folder; see {@link CreateProjectRequestV2}. */
    description?: string
}

/**
 * `PUT /mgmt-api/projects/<name>/description` and
 * `PUT /mgmt-api/projects/<name>/sessions/<session>/description`: the new
 * text of the folder's `README.md`; an empty text removes the file. The
 * owner and the members may change it. Answers the project or the session.
 */
export interface SetDescriptionRequestV2 {
    description: string
}

/** The longest description the API takes, in characters. */
export const DESCRIPTION_MAX_LENGTH = 10_000

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
 * Server-built links and transfer commands for a browsed path. Which keys
 * apply depends on the resource, and the server answers `null` (or leaves out)
 * the ones that don't: the browsed path itself carries the app links plus
 * rsync/sftp/SMB; a listing row carries only the "Open in" columns (Nextcloud
 * and JupyterHub for a folder, the image viewer for a file, LiberTEM and the
 * SampleDB/ElabFTW searches for both). Built on the server because the
 * searches must encode the path exactly as the legacy UI did (#202).
 */
export interface BrowseAccess {
    nextcloud?: string | null
    jupyter?: string | null
    libertem?: string | null
    imageviewer?: string | null
    sampledb?: string | null
    sampledb_erc3?: string | null
    elabftw_erc3?: string | null
    rsync?: string | null
    sftp?: string | null
    smb_windows?: string | null
    smb_unix?: string | null
}

/** One entry of a folder listing. */
export interface BrowseEntry {
    name: string
    /** Normalized absolute path: what to browse to next. */
    path: string
    isdir: boolean
    /** Modification time, epoch seconds; null if the file server reported none. */
    mtime: number | null
    /** Bytes; null for a folder. */
    size: number | null
    access: BrowseAccess
}

/**
 * `GET /mgmt-api/browse?path=&limit=&cursor=`: the browsed path itself and,
 * for a folder, one page of its `entries` (folders first, then by name
 * ignoring case). Pages are keyed, not offset: `next_cursor` is passed back as
 * `cursor` for the page after, until it is null, so an entry added or removed
 * while paging neither repeats nor hides another. `path` is normalized; the
 * loader redirects to it when the URL spelled it differently. A missing path
 * is a 404, a folder the user may not read a 403 (see `browse.loader.ts`).
 */
export interface BrowseV2 {
    path: string
    /** The last segment of `path`; empty for the root. */
    name: string
    isdir: boolean
    /** The project the path is in (its second segment), or null above that. */
    project: string | null
    mtime: number | null
    size: number | null
    access: BrowseAccess
    entries: BrowseEntry[]
    /** How many entries the folder has in all, across every page. */
    total: number
    next_cursor: string | null
}
