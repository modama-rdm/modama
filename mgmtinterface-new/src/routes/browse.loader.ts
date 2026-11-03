import {redirect, type LoaderFunctionArgs} from 'react-router-dom'
import {getCurrentUser} from '../api/auth'
import {ApiError, Unauthorized} from '../api/errors'
import {apiFetchV2} from '../api/clientV2'
import type {BrowseV2, ProjectDetailsV2} from '../api/types'
import {canonicalProjectPath, descriptionPath} from '../lib/projectsUtil'
import {browsePath} from '../lib/quotePath'

/**
 * Entries per page. Most session folders have fewer, so they arrive in one
 * answer; a bigger folder shows a "Load more" button for the pages after.
 */
export const BROWSE_PAGE_SIZE = 200

/** `GET /mgmt-api/browse` for a path, and with `cursor` for its next page. */
export function browseRequest(path: string, cursor?: string): string {
    const query = `path=${encodeURIComponent(path)}&limit=${BROWSE_PAGE_SIZE}`
    return cursor ? `/browse?${query}&cursor=${encodeURIComponent(cursor)}` : `/browse?${query}`
}

/** A project's or a session's description, shown above its folder's listing. */
export interface FolderReadme {
    text: string
    /** Where to change it, for the owner and the members; null for anyone else. */
    editTo: string | null
}

export type BrowseLoaderData =
    | {
          /** The browsed path, as the API normalized it. */
          path: string
          result: BrowseV2
          error: null
          /**
           * The README.md of a project or session folder, null when there is
           * none to look up. Not awaited, so the listing does not wait for it.
           */
          readme: Promise<FolderReadme | null> | null
      }
    | {
          /** The path that was asked for. */
          path: string
          result: null
          /** 404 (no such path) or 403 (a folder this user may not read). */
          error: {status: number; message: string}
      }

/**
 * Loads the first page of a path in the data browser from `GET /mgmt-api/browse`.
 * The screen pages further on its own (see `Browse.tsx`).
 *
 * A path that does not exist, or that the user may not read, is not a throw:
 * the screen renders that state itself, with the breadcrumb of the requested
 * path as the way back up. `Unauthorized` redirects to login; anything else
 * (the file server down, say) goes to the route error boundary.
 *
 * The API normalizes the path (a trailing slash, a `..`) and answers the one
 * spelling; the loader redirects to it, so the address bar, the breadcrumb and
 * the copy buttons agree. Awaited, not streamed: one listing call that
 * usually answers in a few milliseconds.
 */
export async function browseLoader({
                                       request,
                                   }: LoaderFunctionArgs): Promise<BrowseLoaderData> {
    const url = new URL(request.url)
    const path = url.searchParams.get('path') || '/'

    let result: BrowseV2
    try {
        result = await apiFetchV2<BrowseV2>(browseRequest(path))
    } catch (err) {
        if (err instanceof Unauthorized) {
            throw redirect(`/login?next=${encodeURIComponent(browsePath(path))}`)
        }
        if (err instanceof ApiError && (err.status === 404 || err.status === 403)) {
            return {path, result: null, error: {status: err.status, message: err.message}}
        }
        throw err
    }

    if (result.path !== path) throw redirect(browsePath(result.path))
    return {path: result.path, result, error: null, readme: folderReadme(result)}
}

/**
 * The description to show above a folder: that of a project folder
 * (`/<root>/<project>`) or of a session folder one level below. The browse
 * API serves no file contents, but the project's details carry both
 * descriptions, read from these very README.md files, so that is one request.
 * Made only when the listing has a README.md, or may have one on a later page.
 */
export function folderReadme(result: BrowseV2): Promise<FolderReadme | null> | null {
    const segments = result.path.split('/').filter(Boolean)
    if (!result.isdir || result.project === null || segments.length < 2 || segments.length > 3) {
        return null
    }
    const listed = result.entries.some((entry) => !entry.isdir && entry.name === 'README.md')
    if (!listed && result.next_cursor === null) return null
    return lookUpReadme(result.project, `/${segments[0]}/${segments[1]}`, segments[2] ?? null)
}

async function lookUpReadme(
    project: string,
    projectPath: string,
    session: string | null,
): Promise<FolderReadme | null> {
    let details: ProjectDetailsV2
    try {
        details = await apiFetchV2<ProjectDetailsV2>(`/projects/${encodeURIComponent(project)}`)
    } catch {
        // Not a project after all (404), or the service is down: the listing
        // stands on its own.
        return null
    }
    // A folder of the same name under another root is not this project.
    if (canonicalProjectPath(details.relpath) !== projectPath) return null
    const text =
        session === null
            ? details.description
            : details.sessions.find((s) => s.name === session)?.description
    if (!text) return null
    const user = getCurrentUser()
    const member = user !== null && (user === details.owner || details.members.includes(user))
    return {text, editTo: member ? descriptionPath(project, session ?? undefined) : null}
}
