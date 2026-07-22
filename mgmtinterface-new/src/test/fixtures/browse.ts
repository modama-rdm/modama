import type {BrowseSuccess} from '../../api/types'

/**
 * A real production directory listing (10 entries, one file), scrubbed (see
 * README.md). Chosen for the exact edge cases #202 (path/quote_path parity) and
 * MR!61 (the colored "Open in" badges) exist for: folder names with spaces
 * ("UW-Jpfbim Krtd"), a period inside a name ("...vgkwv.fvs"), and every access
 * link the backend can return (nextcloud/jupyter/libertem/sampledb/sampledb_erc3/
 * elabftw_erc3/rsync/sftp/smb_*). Access URLs point at `example.invalid` — the
 * journeys assert on the badge/label, not the real internal hostnames.
 */
export const FIXTURE_BROWSE: BrowseSuccess = {
    "errno": 0,
    "path": "/hdtms/ljfmguujxbnwc",
    "info": {
        "created": "2026-06-03T14:45:15Z",
        "isdir": true,
        "member_count": 10,
        "modified": "Fri, 29 May 2026 09:20:09 GMT",
        "name": "ljfmguujxbnwc",
        "project": "ljfmguujxbnwc",
        "size": null,
        "truncated": false
    },
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
    "path_pyramid": [
        [
            "/",
            "er-c-data"
        ],
        [
            "/hdtms",
            "hdtms"
        ],
        [
            "/hdtms/ljfmguujxbnwc",
            "ljfmguujxbnwc"
        ]
    ],
    "members": [
        {
            "access": {},
            "created": null,
            "isdir": true,
            "modified": null,
            "name": "..",
            "path": "/hdtms",
            "size": null
        },
        {
            "access": {
                "elabftw_erc3": "https://example.invalid/elabftw_erc3",
                "jupyter": "https://example.invalid/jupyter",
                "libertem": "https://example.invalid/libertem",
                "nextcloud": "https://example.invalid/nextcloud",
                "sampledb": "https://example.invalid/sampledb",
                "sampledb_erc3": "https://example.invalid/sampledb_erc3"
            },
            "created": "2026-06-03T14:45:15Z",
            "isdir": true,
            "modified": "Wed, 14 Jan 2026 16:06:54 GMT",
            "name": "Yssrr-Zgwyax-rpbbrzwoktf",
            "path": "/hdtms/ljfmguujxbnwc/Yssrr-Zgwyax-rpbbrzwoktf",
            "size": null
        },
        {
            "access": {
                "elabftw_erc3": "https://example.invalid/elabftw_erc3",
                "jupyter": "https://example.invalid/jupyter",
                "libertem": "https://example.invalid/libertem",
                "nextcloud": "https://example.invalid/nextcloud",
                "sampledb": "https://example.invalid/sampledb",
                "sampledb_erc3": "https://example.invalid/sampledb_erc3"
            },
            "created": "2026-06-03T14:45:15Z",
            "isdir": true,
            "modified": "Tue, 04 Jun 2024 19:47:15 GMT",
            "name": "UW-Jpfbim Krtd",
            "path": "/hdtms/ljfmguujxbnwc/UW-Jpfbim Krtd",
            "size": null
        },
        {
            "access": {
                "elabftw_erc3": "https://example.invalid/elabftw_erc3",
                "jupyter": "https://example.invalid/jupyter",
                "libertem": "https://example.invalid/libertem",
                "nextcloud": "https://example.invalid/nextcloud",
                "sampledb": "https://example.invalid/sampledb",
                "sampledb_erc3": "https://example.invalid/sampledb_erc3"
            },
            "created": "2026-06-03T14:45:15Z",
            "isdir": true,
            "modified": "Tue, 02 Jun 2026 12:35:19 GMT",
            "name": "KKS Ljdcy",
            "path": "/hdtms/ljfmguujxbnwc/KKS Ljdcy",
            "size": null
        },
        {
            "access": {
                "elabftw_erc3": "https://example.invalid/elabftw_erc3",
                "jupyter": "https://example.invalid/jupyter",
                "libertem": "https://example.invalid/libertem",
                "nextcloud": "https://example.invalid/nextcloud",
                "sampledb": "https://example.invalid/sampledb",
                "sampledb_erc3": "https://example.invalid/sampledb_erc3"
            },
            "created": "2026-06-03T14:45:15Z",
            "isdir": true,
            "modified": "Fri, 20 Feb 2026 10:36:16 GMT",
            "name": "SAA Nvvkiqkbb 9119-6663",
            "path": "/hdtms/ljfmguujxbnwc/SAA Nvvkiqkbb 9119-6663",
            "size": null
        },
        {
            "access": {
                "elabftw_erc3": "https://example.invalid/elabftw_erc3",
                "jupyter": "https://example.invalid/jupyter",
                "libertem": "https://example.invalid/libertem",
                "nextcloud": "https://example.invalid/nextcloud",
                "sampledb": "https://example.invalid/sampledb",
                "sampledb_erc3": "https://example.invalid/sampledb_erc3"
            },
            "created": "2026-06-03T14:45:15Z",
            "isdir": true,
            "modified": "Tue, 02 Jun 2026 12:36:51 GMT",
            "name": "Fxfwmyq 9938",
            "path": "/hdtms/ljfmguujxbnwc/Fxfwmyq 9938",
            "size": null
        },
        {
            "access": {
                "elabftw_erc3": "https://example.invalid/elabftw_erc3",
                "jupyter": "https://example.invalid/jupyter",
                "libertem": "https://example.invalid/libertem",
                "nextcloud": "https://example.invalid/nextcloud",
                "sampledb": "https://example.invalid/sampledb",
                "sampledb_erc3": "https://example.invalid/sampledb_erc3"
            },
            "created": "2026-06-03T14:45:15Z",
            "isdir": true,
            "modified": "Thu, 16 May 2024 10:54:18 GMT",
            "name": "Vfnmmmkdph EK-K 8.0",
            "path": "/hdtms/ljfmguujxbnwc/Vfnmmmkdph EK-K 8.0",
            "size": null
        },
        {
            "access": {
                "elabftw_erc3": "https://example.invalid/elabftw_erc3",
                "jupyter": "https://example.invalid/jupyter",
                "libertem": "https://example.invalid/libertem",
                "nextcloud": "https://example.invalid/nextcloud",
                "sampledb": "https://example.invalid/sampledb",
                "sampledb_erc3": "https://example.invalid/sampledb_erc3"
            },
            "created": "2026-06-03T14:45:15Z",
            "isdir": true,
            "modified": "Tue, 11 Feb 2025 13:34:00 GMT",
            "name": "Bnnyccyz hdy ygas qltbg",
            "path": "/hdtms/ljfmguujxbnwc/Bnnyccyz hdy ygas qltbg",
            "size": null
        },
        {
            "access": {
                "elabftw_erc3": "https://example.invalid/elabftw_erc3",
                "jupyter": "https://example.invalid/jupyter",
                "libertem": "https://example.invalid/libertem",
                "nextcloud": "https://example.invalid/nextcloud",
                "sampledb": "https://example.invalid/sampledb",
                "sampledb_erc3": "https://example.invalid/sampledb_erc3"
            },
            "created": "2026-06-03T14:45:15Z",
            "isdir": true,
            "modified": "Mon, 01 Dec 2025 12:23:17 GMT",
            "name": "rbfoq",
            "path": "/hdtms/ljfmguujxbnwc/rbfoq",
            "size": null
        },
        {
            "access": {
                "elabftw_erc3": "https://example.invalid/elabftw_erc3",
                "jupyter": "https://example.invalid/jupyter",
                "libertem": "https://example.invalid/libertem",
                "nextcloud": "https://example.invalid/nextcloud",
                "sampledb": "https://example.invalid/sampledb",
                "sampledb_erc3": "https://example.invalid/sampledb_erc3"
            },
            "created": "2026-06-03T14:45:15Z",
            "isdir": true,
            "modified": "Fri, 22 Aug 2025 10:26:20 GMT",
            "name": "qyojxv",
            "path": "/hdtms/ljfmguujxbnwc/qyojxv",
            "size": null
        },
        {
            "access": {
                "elabftw_erc3": "https://example.invalid/elabftw_erc3",
                "imageviewer": "https://example.invalid/imageviewer",
                "libertem": "https://example.invalid/libertem",
                "sampledb": "https://example.invalid/sampledb",
                "sampledb_erc3": "https://example.invalid/sampledb_erc3"
            },
            "created": "2025-02-07T15:00:06Z",
            "isdir": false,
            "modified": "Fri, 07 Feb 2025 15:00:01 GMT",
            "name": "Syqlphpy cgr rhxx vgkwv.fvs",
            "path": "/hdtms/ljfmguujxbnwc/Syqlphpy cgr rhxx vgkwv.fvs",
            "size": "471631"
        }
    ]
}
