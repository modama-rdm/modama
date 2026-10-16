import type {IngestShareV2} from '../api/types'

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
 * The host and workgroup come with every share from the API (#172), whether it
 * was just opened or read from the list; both stay injectable for tests.
 */
export function shareAccess(
    share: IngestShareV2,
    host: string = share.smb_host,
    workgroup: string = share.workgroup,
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
