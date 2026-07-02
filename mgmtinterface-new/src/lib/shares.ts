import type {SerializedShare} from '../api/types'

/**
 * SMB host + workgroup for ingest shares. The `/api/share/` payload omits these —
 * the backend only injects them in `add_share_info` when rendering its own
 * templates — so the SPA carries the same constants to build the connection
 * details. TODO(api, #172): have the share API return these instead of pinning
 * infra config in the client.
 */
export const INGEST_SMB_HOST = 'moellenstedt.iff.kfa-juelich.de'
export const INGEST_WORKGROUP = 'ER-C-INGEST'

export interface ShareAccess {
    /** Windows Explorer address, `\\host\share`. */
    winAddress: string
    /** Windows username, `WORKGROUP\user`. */
    winUsername: string
    /** `net use` command that maps the share to drive Z:. */
    winMap: string
    /** `net use` command that unmaps drive Z:. */
    winUnmap: string
    /** `net use` command that unmaps a share opened via Explorer. */
    winUnmapExplorer: string
    /** Linux / macOS `smb://` address. */
    unixAddress: string
    /** Temporary password (same for both platforms). */
    password: string
}

/**
 * Build the copy-ready connection strings for an active ingest share — the
 * client-side equivalent of the fields in the legacy `sharedataset.html.j2`.
 * `host`/`workgroup` default to the ingest constants but are injectable for tests.
 */
export function shareAccess(
    share: SerializedShare,
    host: string = INGEST_SMB_HOST,
    workgroup: string = INGEST_WORKGROUP,
): ShareAccess {
    const unc = `\\\\${host}\\${share.share_name}`
    const user = `${workgroup}\\${share.username}`
    return {
        winAddress: unc,
        winUsername: user,
        winMap: `net use /persistent:no z: ${unc} ${share.password} /USER:${user}`,
        winUnmap: 'net use z: /delete',
        winUnmapExplorer: `net use ${unc} /delete`,
        unixAddress: `smb://${share.username}@${host}/${share.share_name}`,
        password: share.password,
    }
}
