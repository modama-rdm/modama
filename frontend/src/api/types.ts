/**
 * TypeScript mirrors of the mgmtinterface `/api/*` JSON shapes.
 *
 * Kept aligned with the backend ground truth in
 * `design-handoff/data-shapes/{permissions,smbdrop}-schema.py`. Timestamps are
 * Unix epoch seconds (floats). The API is frozen for the migration, so these
 * shapes should only change when the backend does.
 */

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
  /** Slow to compute; may be null until resolved. */
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

/* ---- Request payloads (used by later screens) ---- */

export interface CreateProjectRequest {
  name: string
  owner: string
  members: string[]
}

export interface CreateDatasetRequest {
  name: string
  owner: string
}

export interface SealDatasetRequest {
  name: string
  user: string
}

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
