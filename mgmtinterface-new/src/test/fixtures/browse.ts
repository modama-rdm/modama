import type {BrowseV2} from '../../api/types'

/**
 * A real production directory listing (10 entries, one file), scrubbed (see
 * README.md): captured on 2026-07-22 from the legacy `/api/browse/` and
 * re-shaped to `GET /mgmt-api/browse`. No `..` row and no breadcrumb (the
 * screen builds both from the path), `mtime` as epoch seconds instead of an
 * RFC 1123 string, `size` as a number, entries folders first and by name
 * ignoring case, and `total`/`next_cursor` for paging (one page here;
 * `world.ts` pages it). Chosen for the exact edge cases #202 (path/quote_path
 * parity) and MR!61 (the colored "Open in" badges) exist for: folder names with
 * spaces ("UW-Jpfbim Krtd"), a period inside a name ("...vgkwv.fvs"), and every
 * access link the backend can return (nextcloud/jupyter/libertem/sampledb/
 * sampledb_erc3/elabftw_erc3/rsync/sftp/smb_*). Access URLs point at
 * `example.invalid` — the journeys assert on the badge/label, not the real
 * internal hostnames.
 */
export const FIXTURE_BROWSE: BrowseV2 = {
    "path": "/hdtms/ljfmguujxbnwc",
    "name": "ljfmguujxbnwc",
    "isdir": true,
    "project": "ljfmguujxbnwc",
    "mtime": 1780046409.0,
    "size": null,
    "access": {
        "elabftw_erc3": "https://example.invalid/elabftw_erc3",
        "jupyter": "https://example.invalid/jupyter",
        "libertem": "https://example.invalid/libertem",
        "nextcloud": "https://example.invalid/nextcloud",
        "rsync": "https://example.invalid/rsync",
        "sampledb": "https://example.invalid/sampledb",
        "sampledb_erc3": "https://example.invalid/sampledb_erc3",
        "sftp": "https://example.invalid/sftp",
        "smb_unix": "https://example.invalid/smb_unix",
        "smb_windows": "https://example.invalid/smb_windows"
    },
    "entries": [
        {
            "name": "Bnnyccyz hdy ygas qltbg",
            "path": "/hdtms/ljfmguujxbnwc/Bnnyccyz hdy ygas qltbg",
            "isdir": true,
            "mtime": 1739280840.0,
            "size": null,
            "access": {
                "elabftw_erc3": "https://example.invalid/elabftw_erc3",
                "jupyter": "https://example.invalid/jupyter",
                "libertem": "https://example.invalid/libertem",
                "nextcloud": "https://example.invalid/nextcloud",
                "sampledb": "https://example.invalid/sampledb",
                "sampledb_erc3": "https://example.invalid/sampledb_erc3"
            }
        },
        {
            "name": "Fxfwmyq 9938",
            "path": "/hdtms/ljfmguujxbnwc/Fxfwmyq 9938",
            "isdir": true,
            "mtime": 1780403811.0,
            "size": null,
            "access": {
                "elabftw_erc3": "https://example.invalid/elabftw_erc3",
                "jupyter": "https://example.invalid/jupyter",
                "libertem": "https://example.invalid/libertem",
                "nextcloud": "https://example.invalid/nextcloud",
                "sampledb": "https://example.invalid/sampledb",
                "sampledb_erc3": "https://example.invalid/sampledb_erc3"
            }
        },
        {
            "name": "KKS Ljdcy",
            "path": "/hdtms/ljfmguujxbnwc/KKS Ljdcy",
            "isdir": true,
            "mtime": 1780403719.0,
            "size": null,
            "access": {
                "elabftw_erc3": "https://example.invalid/elabftw_erc3",
                "jupyter": "https://example.invalid/jupyter",
                "libertem": "https://example.invalid/libertem",
                "nextcloud": "https://example.invalid/nextcloud",
                "sampledb": "https://example.invalid/sampledb",
                "sampledb_erc3": "https://example.invalid/sampledb_erc3"
            }
        },
        {
            "name": "qyojxv",
            "path": "/hdtms/ljfmguujxbnwc/qyojxv",
            "isdir": true,
            "mtime": 1755858380.0,
            "size": null,
            "access": {
                "elabftw_erc3": "https://example.invalid/elabftw_erc3",
                "jupyter": "https://example.invalid/jupyter",
                "libertem": "https://example.invalid/libertem",
                "nextcloud": "https://example.invalid/nextcloud",
                "sampledb": "https://example.invalid/sampledb",
                "sampledb_erc3": "https://example.invalid/sampledb_erc3"
            }
        },
        {
            "name": "rbfoq",
            "path": "/hdtms/ljfmguujxbnwc/rbfoq",
            "isdir": true,
            "mtime": 1764591797.0,
            "size": null,
            "access": {
                "elabftw_erc3": "https://example.invalid/elabftw_erc3",
                "jupyter": "https://example.invalid/jupyter",
                "libertem": "https://example.invalid/libertem",
                "nextcloud": "https://example.invalid/nextcloud",
                "sampledb": "https://example.invalid/sampledb",
                "sampledb_erc3": "https://example.invalid/sampledb_erc3"
            }
        },
        {
            "name": "SAA Nvvkiqkbb 9119-6663",
            "path": "/hdtms/ljfmguujxbnwc/SAA Nvvkiqkbb 9119-6663",
            "isdir": true,
            "mtime": 1771583776.0,
            "size": null,
            "access": {
                "elabftw_erc3": "https://example.invalid/elabftw_erc3",
                "jupyter": "https://example.invalid/jupyter",
                "libertem": "https://example.invalid/libertem",
                "nextcloud": "https://example.invalid/nextcloud",
                "sampledb": "https://example.invalid/sampledb",
                "sampledb_erc3": "https://example.invalid/sampledb_erc3"
            }
        },
        {
            "name": "UW-Jpfbim Krtd",
            "path": "/hdtms/ljfmguujxbnwc/UW-Jpfbim Krtd",
            "isdir": true,
            "mtime": 1717530435.0,
            "size": null,
            "access": {
                "elabftw_erc3": "https://example.invalid/elabftw_erc3",
                "jupyter": "https://example.invalid/jupyter",
                "libertem": "https://example.invalid/libertem",
                "nextcloud": "https://example.invalid/nextcloud",
                "sampledb": "https://example.invalid/sampledb",
                "sampledb_erc3": "https://example.invalid/sampledb_erc3"
            }
        },
        {
            "name": "Vfnmmmkdph EK-K 8.0",
            "path": "/hdtms/ljfmguujxbnwc/Vfnmmmkdph EK-K 8.0",
            "isdir": true,
            "mtime": 1715856858.0,
            "size": null,
            "access": {
                "elabftw_erc3": "https://example.invalid/elabftw_erc3",
                "jupyter": "https://example.invalid/jupyter",
                "libertem": "https://example.invalid/libertem",
                "nextcloud": "https://example.invalid/nextcloud",
                "sampledb": "https://example.invalid/sampledb",
                "sampledb_erc3": "https://example.invalid/sampledb_erc3"
            }
        },
        {
            "name": "Yssrr-Zgwyax-rpbbrzwoktf",
            "path": "/hdtms/ljfmguujxbnwc/Yssrr-Zgwyax-rpbbrzwoktf",
            "isdir": true,
            "mtime": 1768406814.0,
            "size": null,
            "access": {
                "elabftw_erc3": "https://example.invalid/elabftw_erc3",
                "jupyter": "https://example.invalid/jupyter",
                "libertem": "https://example.invalid/libertem",
                "nextcloud": "https://example.invalid/nextcloud",
                "sampledb": "https://example.invalid/sampledb",
                "sampledb_erc3": "https://example.invalid/sampledb_erc3"
            }
        },
        {
            "name": "Syqlphpy cgr rhxx vgkwv.fvs",
            "path": "/hdtms/ljfmguujxbnwc/Syqlphpy cgr rhxx vgkwv.fvs",
            "isdir": false,
            "mtime": 1738940401.0,
            "size": 471631,
            "access": {
                "elabftw_erc3": "https://example.invalid/elabftw_erc3",
                "imageviewer": "https://example.invalid/imageviewer",
                "libertem": "https://example.invalid/libertem",
                "sampledb": "https://example.invalid/sampledb",
                "sampledb_erc3": "https://example.invalid/sampledb_erc3"
            }
        }
    ],
    "total": 10,
    "next_cursor": null
}
