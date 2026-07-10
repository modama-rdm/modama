import {Link} from 'react-router-dom'
import {Card} from '../components/ui/Card'
import {ArrowLeftIcon} from '../components/ui/icons'
import {CHANGELOG} from '../lib/changelog'

/**
 * What's-new / changelog screen. Renders the typed history from
 * `lib/changelog.ts` (the single source), newest-first, so the team can follow
 * progress while the new UI runs alongside the legacy portal. Reachable from the
 * version badge in the footer.
 */
export function Changelog() {
    return (
        <div className="mx-auto max-w-150 px-6 py-8 animate-[fadeIn_0.2s_ease-out]">
            <Link
                to="/projects"
                className="mb-5 inline-flex items-center gap-1.5 text-sm text-fg-subtle hover:text-fg"
            >
                <ArrowLeftIcon className="h-4 w-4"/>
                Projects
            </Link>

            <h1 className="text-2xl font-bold text-fg">Changelog</h1>
            <p className="mt-1 text-sm text-fg-subtle">
                What’s changed in the Data Management portal.
            </p>

            <ol className="mt-8 space-y-4">
                {CHANGELOG.map((entry) => (
                    <li key={entry.version}>
                        <Card className="p-5">
                            <div className="flex items-baseline justify-between gap-3">
                                <h2 className="flex items-baseline gap-2.5">
                                    <span className="font-mono text-sm font-semibold text-primary">
                                        v{entry.version}
                                    </span>
                                    <span className="text-[15px] font-semibold text-fg">
                                        {entry.title}
                                    </span>
                                </h2>
                                {entry.date && (
                                    <time
                                        dateTime={entry.date}
                                        className="shrink-0 font-mono text-xs text-fg-faint"
                                    >
                                        {entry.date}
                                    </time>
                                )}
                            </div>
                            <ul className="mt-3 list-disc space-y-1.5 pl-5 text-sm text-fg-muted marker:text-fg-faint">
                                {entry.changes.map((change, i) => (
                                    <li key={i}>{change}</li>
                                ))}
                            </ul>
                        </Card>
                    </li>
                ))}
            </ol>
        </div>
    )
}
