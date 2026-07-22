import {createBrowserRouter} from 'react-router-dom'
import {basename, routes} from './routeTree'

/**
 * The app's browser router. The route tree itself lives in `routeTree.tsx` so the
 * journey tests can mount the same routes under a memory router.
 */
export const router = createBrowserRouter(routes, {basename})
