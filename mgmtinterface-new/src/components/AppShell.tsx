import {Link, Outlet, useNavigate} from 'react-router-dom'
import {getCurrentUser, logout} from '../api/auth'
import {NavigationProgress} from './NavigationProgress'
import {Avatar} from './ui/Avatar'
import {useToast} from './ui/toast-context'

/**
 * Authenticated app shell: fixed header with branding, the app name, and a
 * session indicator (current user and logout). Renders the active route below.
 * Mirrors the behaviour of the legacy `_header.html.j2`.
 */
export function AppShell() {
    const navigate = useNavigate()
    const {notify} = useToast()
    const user = getCurrentUser() ?? ''

    function onLogout() {
        logout()
        notify('You were logged out.')
        navigate('/login')
    }

    return (
        <div className="min-h-screen bg-bg">
            <NavigationProgress/>
            <header
                className="fixed inset-x-0 top-0 z-50 flex h-14 items-center gap-3.5 border-b border-default bg-surface px-7">
                <Link to="/projects"
                      className="flex items-center rounded-md focus:outline-none focus:shadow-[0_0_0_3px_var(--ring)]">
                    <img src={`${import.meta.env.BASE_URL}modama.svg`} alt="modama — back to Projects"
                         className="h-6.5"/>
                </Link>
                <div className="h-5.5 w-px shrink-0 bg-default"/>
                <span className="text-[13px] font-medium text-fg-subtle">
          Data Management Self-Service
        </span>
                <div className="flex-1"/>
                <div className="flex items-center gap-2.5">
                    <Avatar username={user}/>
                    <span className="text-sm font-medium text-fg-muted">{user}</span>
                    <span className="inline-block h-4.5 w-px shrink-0 bg-default"/>
                    <button
                        onClick={onLogout}
                        className="rounded-md border border-default bg-transparent px-3 py-1.5 text-[13px] font-medium text-fg-subtle transition-colors hover:bg-surface-muted hover:text-fg-muted"
                    >
                        Log out
                    </button>
                </div>
            </header>
            <main className="pt-14">
                <Outlet/>
            </main>
        </div>
    )
}
