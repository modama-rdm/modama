import {
    createBrowserRouter,
    redirect,
    type LoaderFunctionArgs,
} from 'react-router-dom'
import {isAuthenticated} from './api/auth'
import {AppShell} from './components/AppShell'
import {RouteError} from './components/RouteError'
import {Login} from './routes/Login'
import {NewProject} from './routes/NewProject'
import {newProjectLoader} from './routes/newProject.loader'
import {NewSession} from './routes/NewSession'
import {Placeholder} from './routes/Placeholder'
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
const basename = import.meta.env.BASE_URL.replace(/\/$/, '') || '/'

/** Gate the authenticated area; bounce to /login preserving the attempted URL. */
function protectedLoader({request}: LoaderFunctionArgs) {
    if (!isAuthenticated()) {
        const url = new URL(request.url)
        const next = url.pathname + url.search
        return redirect(`/login?next=${encodeURIComponent(next)}`)
    }
    return null
}

export const router = createBrowserRouter([
    {path: '/login', element: <Login/>, errorElement: <RouteError/>},
    {
        element: <AppShell/>,
        loader: protectedLoader,
        errorElement: <RouteError/>,
        children: [
            {index: true, loader: () => redirect('/projects')},
            {path: 'projects', loader: projectsLoader, element: <Projects/>},
            {path: 'projects/new', loader: newProjectLoader, element: <NewProject/>},
            {
                path: 'projects/:name',
                loader: projectDetailsLoader,
                element: <ProjectDetails/>,
            },
            {
                path: 'projects/:name/members',
                element: <Placeholder title="Manage Members"/>,
            },
            {path: 'projects/:name/sessions/new', element: <NewSession/>},
            {
                path: 'projects/:name/sessions/:dataset/seal',
                element: <SealSession/>,
            },
            {
                path: 'projects/:name/sessions/:dataset/ingest',
                element: <Placeholder title="Ingest Share"/>,
            },
        ],
    },
], {basename})
