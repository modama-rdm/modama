import type {IngestShareV2, SerializedShare} from '../api/types'

/**
 * SMB host + workgroup for ingest shares when the share does not say. The new
 * API's create response (#172) carries them as `smb_host`/`workgroup`; the
 * legacy `GET /api/share/` list, which the ingest screen still reads an existing
 * share from, omits them (the Flask backend only injected them into its own
 * templates). These constants cover that case until the list moves over too.
 */
export const INGEST_SMB_HOST = 'moellenstedt.iff.kfa-juelich.de'
export const INGEST_WORKGROUP = 'ER-C-INGEST'

/** A share from either API: the connection host is known for the new one only. */
export type ConnectableShare = SerializedShare &
    Partial<Pick<IngestShareV2, 'smb_host' | 'workgroup'>>

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
 * `host`/`workgroup` default to what the share itself names, then to the
 * ingest constants; both are injectable for tests.
 */
export function shareAccess(
    share: ConnectableShare,
    host: string = share.smb_host ?? INGEST_SMB_HOST,
    workgroup: string = share.workgroup ?? INGEST_WORKGROUP,
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
