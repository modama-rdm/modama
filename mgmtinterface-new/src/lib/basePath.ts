/**
 * Strip the router basename off an absolute pathname so it becomes an
 * app-relative path. React Router's loader `request.url` is the raw browser URL,
 * so its pathname still carries the basename (e.g. /new/projects). Feeding that
 * straight into `navigate()` after login re-prepends the basename and doubles it
 * up into /new/new/projects — a 404. Callers get back a leading-slash path the
 * router can safely re-mount.
 */
export function stripBasename(pathname: string, base: string): string {
    if (base === '/' || base === '') return pathname
    if (pathname === base) return '/'
    if (pathname.startsWith(base + '/')) return pathname.slice(base.length)
    return pathname
}
