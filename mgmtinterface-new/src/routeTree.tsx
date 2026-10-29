import {redirect, type LoaderFunctionArgs, type RouteObject} from 'react-router-dom'
import {isAuthenticated} from './api/auth'
import {stripBasename} from './lib/basePath'
import {AppShell} from './components/AppShell'
import {RouteError} from './components/RouteError'
import {Browse} from './routes/Browse'
import {browseLoader} from './routes/browse.loader'
import {Changelog} from './routes/Changelog'
import {EditDescription} from './routes/EditDescription'
import {editDescriptionLoader} from './routes/editDescription.loader'
import {IngestShare} from './routes/IngestShare'
import {ingestShareLoader} from './routes/ingestShare.loader'
import {Login} from './routes/Login'
import {ManageMembers} from './routes/ManageMembers'
import {manageMembersLoader} from './routes/manageMembers.loader'
import {NewProject} from './routes/NewProject'
import {newProjectLoader} from './routes/newProject.loader'
import {NewSession} from './routes/NewSession'
import {ProjectDetails} from './routes/ProjectDetails'
import {projectDetailsLoader} from './routes/projectDetails.loader'
import {SealSession} from './routes/SealSession'
import {Projects} from './routes/Projects.tsx'
import {projectsLoader} from './routes/projects.loader'

/**
 * The router's basename, derived from the Vite `base` (set per-environment via
 * VITE_BASE_PATH). Vite exposes that value as import.meta.env.BASE_URL with a
 * trailing slash ('/' in dev, '/new/' in the production build); React Router
 * wants it without one. Deriving it here keeps every in-app path (`/projects`,
 * `/login`, …) sub-path agnostic — the router prepends the basename, so nothing
 * else in the app needs to know where it's mounted.
 */
export const basename = import.meta.env.BASE_URL.replace(/\/$/, '') || '/'

/** Gate the authenticated area; bounce to /login preserving the attempted URL. */
function protectedLoader({request}: LoaderFunctionArgs) {
    if (!isAuthenticated()) {
        const url = new URL(request.url)
        const next = stripBasename(url.pathname, basename) + url.search
        return redirect(`/login?next=${encodeURIComponent(next)}`)
    }
    return null
}

/**
 * The route tree, kept separate from the browser router in `router.tsx` so the
 * journey tests can mount the same routes and loaders under a `createMemoryRouter`
 * at an arbitrary entry. Any route added here is covered by both.
 */
export const routes: RouteObject[] = [
    {path: '/login', element: <Login/>, errorElement: <RouteError/>},
    {
        element: <AppShell/>,
        loader: protectedLoader,
        errorElement: <RouteError/>,
        children: [
            {index: true, loader: () => redirect('/projects')},
            {path: 'browse', loader: browseLoader, element: <Browse/>},
            {path: 'changelog', element: <Changelog/>},
            {path: 'projects', loader: projectsLoader, element: <Projects/>},
            {path: 'projects/new', loader: newProjectLoader, element: <NewProject/>},
            {
                path: 'projects/:name',
                loader: projectDetailsLoader,
                element: <ProjectDetails/>,
            },
            {
                path: 'projects/:name/members',
                loader: manageMembersLoader,
                element: <ManageMembers/>,
            },
            {
                path: 'projects/:name/description',
                loader: editDescriptionLoader,
                element: <EditDescription/>,
            },
            {path: 'projects/:name/sessions/new', element: <NewSession/>},
            {
                path: 'projects/:name/sessions/:dataset/seal',
                element: <SealSession/>,
            },
            {
                path: 'projects/:name/sessions/:dataset/description',
                loader: editDescriptionLoader,
                element: <EditDescription/>,
            },
            {
                path: 'projects/:name/sessions/:dataset/ingest',
                loader: ingestShareLoader,
                element: <IngestShare/>,
            },
        ],
    },
]
