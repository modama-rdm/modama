import {redirect, type LoaderFunctionArgs} from 'react-router-dom'
import {Unauthorized, apiFetch} from '../api/client'
import type {BrowseResponse} from '../api/types'
import {browsePath} from '../lib/quotePath'

export interface BrowseLoaderData {
    /** The path that was requested (from `?path=`); defaults to root. */
    path: string
    /** Listing or an `errno` error envelope (path-not-found is not a throw). */
    result: BrowseResponse
}

/**
 * Loads a WebDAV directory/file listing for the data browser via the stopgap
 * Flask `/api/browse/` endpoint. A not-found path comes back as an `errno`
 * envelope (HTTP 200), so only `Unauthorized` redirects to login here — the
 * screen renders the error state itself. Awaited (not streamed): a single,
 * usually-quick listing call, unlike the eager `usage_bytes` computation the
 * project screens stream behind a skeleton.
 */
export async function browseLoader({
                                       request,
                                   }: LoaderFunctionArgs): Promise<BrowseLoaderData> {
    const url = new URL(request.url)
    const path = url.searchParams.get('path') || '/'
    const next = browsePath(path)
    try {
        const result = await apiFetch<BrowseResponse>(
            `/api/browse/?path=${encodeURIComponent(path)}`,
        )
        return {path, result}
    } catch (err) {
        if (err instanceof Unauthorized) {
            throw redirect(`/login?next=${encodeURIComponent(next)}`)
        }
        throw err
    }
}
