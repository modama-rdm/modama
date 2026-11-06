import {Link} from 'react-router-dom'
import {HeartIcon} from './ui/icons'

/**
 * App footer: a quiet credit line and the current app version, which links to
 * the changelog. Kept deliberately understated (muted, small type) so it reads
 * as a signature, not a banner. The version comes from `__APP_VERSION__`
 * (injected from package.json at build time - see vite.config.ts), so it always
 * matches the shipped build.
 */
export function Footer() {
    return (
        <footer className="border-t border-default bg-surface py-4">
            {/* The same column as the header: both line up with the edges of the cards. */}
            <div
                className="mx-auto flex max-w-270 flex-col items-center justify-between gap-2 px-6 text-[13px] text-fg-subtle sm:flex-row">
                <span className="inline-flex items-center gap-1.5">
                    Made with
                    <HeartIcon className="h-3.5 w-3.5 text-danger"/>
                    <span className="sr-only">love</span>
                    in ER-C
                </span>
                <Link
                    to="/changelog"
                    className="rounded font-mono text-fg-subtle transition-colors hover:text-fg-muted focus:outline-none focus:shadow-[0_0_0_3px_var(--ring)]"
                >
                    v{__APP_VERSION__}
                </Link>
            </div>
        </footer>
    )
}
