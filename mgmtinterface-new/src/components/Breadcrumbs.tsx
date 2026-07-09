import {Fragment} from 'react'
import {Link} from 'react-router-dom'

export interface Crumb {
    label: string
    /** Link target; the last crumb (current page) has none. */
    to?: string
    /** Render in the monospace project/session style. */
    mono?: boolean
}

/**
 * `Projects › project › Page`. Every crumb but the last links back up the
 * hierarchy; the last one names the current page.
 */
export function Breadcrumbs({crumbs, className = 'mb-7'}: {
    crumbs: Crumb[]
    className?: string
}) {
    const mono = 'font-mono text-xs'
    return (
        <nav className={`flex flex-wrap items-center gap-1.5 text-[13px] ${className}`}>
            {crumbs.map((c, i) => (
                <Fragment key={`${c.label}-${i}`}>
                    {i > 0 && <span className="text-fg-faint">›</span>}
                    {c.to ? (
                        <Link
                            to={c.to}
                            className={`text-primary hover:underline ${c.mono ? mono : ''}`}
                        >
                            {c.label}
                        </Link>
                    ) : (
                        <span className={`font-medium text-fg-muted ${c.mono ? mono : ''}`}>
                            {c.label}
                        </span>
                    )}
                </Fragment>
            ))}
        </nav>
    )
}
