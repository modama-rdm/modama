/**
 * What the data browser derives from a path on its own. The browse API answers
 * the normalized path and nothing that follows from it: the breadcrumb trail
 * and the parent folder are computed here, as the legacy `get_path_pyramid`
 * did on the server.
 */

/** The label of the root crumb, as on the legacy browse page. */
export const ROOT_LABEL = 'er-c-data'

/**
 * Breadcrumb trail of a normalized path: `[fullPath, label]` from the root
 * down to the path itself. `/adhoc/proj` -> root, `/adhoc`, `/adhoc/proj`.
 */
export function pathPyramid(path: string): [string, string][] {
    const crumbs: [string, string][] = [['/', ROOT_LABEL]]
    let full = ''
    for (const segment of path.split('/').filter(Boolean)) {
        full += `/${segment}`
        crumbs.push([full, segment])
    }
    return crumbs
}

/** The folder a normalized path is in, or null at the root. */
export function parentOf(path: string): string | null {
    if (path === '/') return null
    const cut = path.lastIndexOf('/')
    return cut <= 0 ? '/' : path.slice(0, cut)
}
