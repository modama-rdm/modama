import {
  createBrowserRouter,
  redirect,
  type LoaderFunctionArgs,
} from 'react-router-dom'
import { isAuthenticated } from './api/auth'
import { AppShell } from './components/AppShell'
import { RouteError } from './components/RouteError'
import { Login } from './routes/Login'
import { Projects } from './routes/Projects'

/** Gate the authenticated area; bounce to /login preserving the attempted URL. */
function protectedLoader({ request }: LoaderFunctionArgs) {
  if (!isAuthenticated()) {
    const url = new URL(request.url)
    const next = url.pathname + url.search
    return redirect(`/login?next=${encodeURIComponent(next)}`)
  }
  return null
}

export const router = createBrowserRouter([
  { path: '/login', element: <Login />, errorElement: <RouteError /> },
  {
    element: <AppShell />,
    loader: protectedLoader,
    errorElement: <RouteError />,
    children: [
      { index: true, loader: () => redirect('/projects') },
      { path: 'projects', element: <Projects /> },
    ],
  },
])
