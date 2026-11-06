import {Link, Outlet, ScrollRestoration, useNavigate} from 'react-router-dom'
import {getCurrentUser, logout} from '../api/auth'
import {useTheme} from '../hooks/useTheme'
import {Footer} from './Footer'
import {NavigationProgress} from './NavigationProgress'
import {Avatar} from './ui/Avatar'
import {MoonIcon, SunIcon} from './ui/icons'
import {useToast} from './ui/toast-context'

/**
 * Authenticated app shell: fixed header with branding, the app name, and a
 * session indicator (current user and logout). Renders the active route below.
 * Mirrors the behaviour of the legacy `_header.html.j2`.
 */
export function AppShell() {
    const navigate = useNavigate()
    const {notify} = useToast()
    const {theme, toggleTheme} = useTheme()
    const user = getCurrentUser() ?? ''

    async function onLogout() {
        // Awaited so the server drops the session cookie before we navigate;
        // `logout` clears the remembered sign-in either way.
        await logout()
        notify('You were logged out.')
        navigate('/login')
    }

    return (
        <div className="flex min-h-screen flex-col bg-bg">
            <NavigationProgress/>
            <header className="fixed inset-x-0 top-0 z-50 h-14 border-b border-default bg-surface">
                {/* The column of the wide pages (Projects, a project, the data
                    browser): the logo and Log out line up with the edges of the
                    cards below. */}
                <div className="mx-auto flex h-full max-w-270 items-center gap-3.5 px-6">
                    {/* modama.svg has empty space around the word: at this height
                        about 6px on the left, which -ml-1.5 takes back so the
                        visible logo starts on the cards' edge. */}
                    <Link to="/projects"
                          className="-ml-1.5 flex items-center rounded-md focus:outline-none focus:shadow-[0_0_0_3px_var(--ring)]">
                        <img src={`${import.meta.env.BASE_URL}modama.svg`} alt="modama — back to Projects"
                             className="h-6.5 dark:invert"/>
                    </Link>
                    <div className="h-5.5 w-px shrink-0 bg-default"/>
                    <span className="text-[13px] font-medium text-fg-subtle">
                        Data Management Self-Service
                    </span>
                    <div className="flex-1"/>
                    <div className="flex items-center gap-2.5">
                        <button
                            onClick={toggleTheme}
                            aria-label={theme === 'dark' ? 'Switch to light theme' : 'Switch to dark theme'}
                            title={theme === 'dark' ? 'Switch to light theme' : 'Switch to dark theme'}
                            className="flex h-8 w-8 items-center justify-center rounded-md text-fg-subtle transition-colors hover:bg-surface-muted hover:text-fg-muted focus:outline-none focus:shadow-[0_0_0_3px_var(--ring)]"
                        >
                            {theme === 'dark' ? <SunIcon className="h-4.5 w-4.5"/> :
                                <MoonIcon className="h-4.5 w-4.5"/>}
                        </button>
                        <span className="inline-block h-4.5 w-px shrink-0 bg-default"/>
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
                </div>
            </header>
            <main className="flex-1 pt-14">
                <Outlet/>
            </main>
            <Footer/>
            {/* A page you open starts at the top, and Back returns you to where
                you were. Without it every page kept the scroll offset of the one
                before, so the changelog, opened from the footer, opened part way
                down. Only navigations move the page: a sort or "Load more" stays. */}
            <ScrollRestoration/>
        </div>
    )
}
