/**
 * Port of the backend's `quote_path` (mgmtinterface/__init__.py), i.e. Python's
 * `quote_plus(path, safe="!$'()*,/:;?@")`: percent-encode the UTF-8 bytes of
 * everything outside `[A-Za-z0-9_.~-]` and the safe set, with space -> `+`.
 *
 * ELN entries (SampleDB/ElabFTW) store browse URLs "as entered" and are
 * searched for byte-by-byte, so links the app produces for a path must encode
 * it exactly like the legacy UI and the backend's search-link builders do —
 * notably `/` stays literal (unlike encodeURIComponent's `%2F`) and `#`
 * becomes `%23`. See python/mgmtinterface/tests/test_url_encoding.py.
 */

const SAFE = new Set("!$'()*,/:;?@ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_.~-")

export function quotePath(path: string): string {
    const bytes = new TextEncoder().encode(path)
    let out = ''
    for (const b of bytes) {
        const ch = String.fromCharCode(b)
        if (SAFE.has(ch)) out += ch
        else if (ch === ' ') out += '+'
        else out += '%' + b.toString(16).toUpperCase().padStart(2, '0')
    }
    return out
}

/** Router-relative link to a path in the data browser. */
export function browsePath(path: string): string {
    return `/browse?path=${quotePath(path)}`
}

/** Absolute URL for a path — what the copy buttons hand out for pasting into
 *  ELN entries. Base handling matches router.tsx's basename. */
export function browseUrl(path: string): string {
    const base = import.meta.env.BASE_URL.replace(/\/$/, '')
    return `${window.location.origin}${base}${browsePath(path)}`
}
