import {redirect, type LoaderFunctionArgs} from 'react-router-dom'
import {ApiError, Unauthorized} from '../api/errors'
import {apiFetchV2} from '../api/clientV2'
import type {BrowseV2} from '../api/types'
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

export type BrowseLoaderData =
    | {
          /** The browsed path, as the API normalized it. */
          path: string
          result: BrowseV2
          error: null
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
    return {path: result.path, result, error: null}
}
