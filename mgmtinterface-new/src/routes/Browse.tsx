import {Fragment, useId, useLayoutEffect, useRef, useState} from 'react'
import {Link, useLoaderData, useNavigate} from 'react-router-dom'
import {Unauthorized} from '../api/errors'
import {apiFetchV2} from '../api/clientV2'
import type {BrowseAccess, BrowseEntry, BrowseV2} from '../api/types'
import {CopyButton, CopyField} from '../components/CopyField'
import {useCopy} from '../hooks/useCopy'
import {Button} from '../components/ui/Button'
import {Card} from '../components/ui/Card'
import {CheckCircleIcon, ChevronDownIcon, FileIcon, FolderIcon} from '../components/ui/icons'
import {formatBytes, formatDateTime} from '../lib/format'
import {parentOf, pathPyramid} from '../lib/paths'
import {browsePath, browseUrl} from '../lib/quotePath'
import {browseRequest, type BrowseLoaderData} from './browse.loader'

/** Bytes as the API reports them, human-readably; "—" for a folder. */
function formatSize(size: number | null): string {
    return size == null ? '—' : formatBytes(size)
}

/** Epoch seconds as the API reports them; "—" if the file server gave none. */
function formatModified(mtime: number | null): string {
    return mtime == null ? '—' : formatDateTime(mtime)
}

/**
 * App-integration links, rendered as colored "Open in" badges (per the Claude
 * Design browse screen). Each carries its own palette: a solid pastel chip in
 * light mode, a translucent tint in dark mode. The color is what makes these
 * read as actionable — the previous faint-gray text was too low-contrast. Order
 * follows the legacy browse page; the two ER-C-3 links extend the palette.
 */
const APP_LINKS: {
    key: keyof BrowseAccess
    label: string
    /** Tailwind badge palette (light pastel + dark translucent + hover). */
    badge: string
}[] = [
    {
        key: 'nextcloud',
        label: 'Nextcloud',
        badge:
            'bg-sky-50 text-sky-700 hover:bg-sky-100 ' +
            'dark:bg-sky-500/15 dark:text-sky-300 dark:hover:bg-sky-500/25',
    },
    {
        key: 'jupyter',
        label: 'JupyterHub',
        badge:
            'bg-orange-50 text-orange-700 hover:bg-orange-100 ' +
            'dark:bg-orange-500/15 dark:text-orange-300 dark:hover:bg-orange-500/25',
    },
    {
        key: 'libertem',
        label: 'LiberTEM',
        badge:
            'bg-green-50 text-green-800 hover:bg-green-100 ' +
            'dark:bg-green-500/15 dark:text-green-300 dark:hover:bg-green-500/25',
    },
    {
        key: 'imageviewer',
        label: 'Image viewer',
        badge:
            'bg-violet-50 text-violet-700 hover:bg-violet-100 ' +
            'dark:bg-violet-500/15 dark:text-violet-300 dark:hover:bg-violet-500/25',
    },
    {
        key: 'sampledb',
        label: 'SampleDB',
        badge:
            'bg-fuchsia-50 text-fuchsia-800 hover:bg-fuchsia-100 ' +
            'dark:bg-fuchsia-500/15 dark:text-fuchsia-300 dark:hover:bg-fuchsia-500/25',
    },
    {
        key: 'sampledb_erc3',
        label: 'SampleDB (ER-C-3)',
        badge:
            'bg-indigo-50 text-indigo-700 hover:bg-indigo-100 ' +
            'dark:bg-indigo-500/15 dark:text-indigo-300 dark:hover:bg-indigo-500/25',
    },
    {
        key: 'elabftw_erc3',
        label: 'ElabFTW (ER-C-3)',
        badge:
            'bg-rose-50 text-rose-700 hover:bg-rose-100 ' +
            'dark:bg-rose-500/15 dark:text-rose-300 dark:hover:bg-rose-500/25',
    },
]

/**
 * A single "open in external app" badge. Renders a real external link (opens in
 * a new tab) styled as the design's colored pill. `size` matches the two places
 * it appears: `sm` for the compact listing-table rows, `md` for the top card.
 */
function AccessBadge({
                         href,
                         label,
                         badge,
                         size,
                         onOpen,
                     }: {
    href: string
    label: string
    badge: string
    size: 'sm' | 'md'
    onOpen?: () => void
}) {
    const dims = size === 'sm' ? 'px-2.5 py-1 text-xs' : 'px-3 py-1.5 text-sm'
    return (
        <a
            href={href}
            target="_blank"
            rel="noreferrer"
            onClick={onOpen}
            className={`inline-flex items-center rounded-md font-semibold transition-colors ${dims} ${badge}`}
        >
            {label}
        </a>
    )
}

/** The badges for every link the server answered for this resource. */
function AccessBadges({access, onOpenApp}: { access: BrowseAccess; onOpenApp: (app: AppKey) => void }) {
    return (
        <>
            {APP_LINKS.filter(({key}) => access[key]).map(({key, label, badge}) => (
                <AccessBadge
                    key={key}
                    href={access[key]!}
                    label={label}
                    badge={badge}
                    size="sm"
                    onOpen={() => onOpenApp(key)}
                />
            ))}
        </>
    )
}

type AppKey = keyof BrowseAccess

const LAST_APP_KEY = 'erc-browse-open-in'

/**
 * The app this browser last opened something in, offered as a one-click button
 * in every listing row that has it: people often open file after file in the
 * same tool. Remembered like the table sorts (`useStoredSort`); without storage
 * it holds for this visit only.
 */
function useLastApp(): [AppKey | null, (app: AppKey) => void] {
    const [last, setLast] = useState<AppKey | null>(() => {
        try {
            const stored = localStorage.getItem(LAST_APP_KEY)
            return APP_LINKS.find(({key}) => key === stored)?.key ?? null
        } catch {
            /* storage unavailable — nothing remembered yet */
            return null
        }
    })

    function remember(app: AppKey) {
        setLast(app)
        try {
            localStorage.setItem(LAST_APP_KEY, app)
        } catch {
            /* storage unavailable — remembered for this visit only */
        }
    }

    return [last, remember]
}

/**
 * A listing row's "Open in" links behind one button, so the row stays one line
 * high instead of stacking five to seven badges. Once an app has been used
 * (`lastApp`), its badge sits before the button as a one-click link and the
 * button shrinks to the arrow; a row without that app keeps "Open in".
 *
 * The panel is `fixed` and placed under the button (above it when the window
 * has no room below), so the card's `overflow-hidden` cannot clip it; it
 * follows the button in the DOM, so Tab goes from the button into the links.
 * Escape, a click outside or opening a link closes it.
 */
function OpenInMenu({
                        access,
                        lastApp,
                        onOpenApp,
                    }: {
    access: BrowseAccess
    lastApp: AppKey | null
    onOpenApp: (app: AppKey) => void
}) {
    const [open, setOpen] = useState(false)
    const buttonRef = useRef<HTMLButtonElement>(null)
    const panelRef = useRef<HTMLDivElement>(null)
    const panelId = useId()
    const links = APP_LINKS.filter(({key}) => access[key])
    const quick = links.find(({key}) => key === lastApp)

    // A layout effect, so the panel is in place before it is first painted.
    useLayoutEffect(() => {
        const button = buttonRef.current
        const panel = panelRef.current
        if (!open || !button || !panel) return
        const place = () => {
            const b = button.getBoundingClientRect()
            const {width, height} = panel.getBoundingClientRect()
            const fitsBelow = b.bottom + 4 + height <= window.innerHeight - 8
            panel.style.top = `${fitsBelow ? b.bottom + 4 : Math.max(8, b.top - 4 - height)}px`
            panel.style.left = `${Math.max(8, b.right - width)}px`
        }
        const onPointerDown = (e: PointerEvent) => {
            const target = e.target as Node
            if (!button.contains(target) && !panel.contains(target)) setOpen(false)
        }
        const onKeyDown = (e: KeyboardEvent) => {
            if (e.key !== 'Escape') return
            setOpen(false)
            button.focus()
        }
        place()
        window.addEventListener('scroll', place, true)
        window.addEventListener('resize', place)
        document.addEventListener('pointerdown', onPointerDown)
        document.addEventListener('keydown', onKeyDown)
        return () => {
            window.removeEventListener('scroll', place, true)
            window.removeEventListener('resize', place)
            document.removeEventListener('pointerdown', onPointerDown)
            document.removeEventListener('keydown', onKeyDown)
        }
    }, [open])

    if (links.length === 0) return null
    return (
        <div className="flex items-stretch gap-1">
            {quick && (
                <AccessBadge
                    href={access[quick.key]!}
                    label={quick.label}
                    badge={quick.badge}
                    size="sm"
                    onOpen={() => onOpenApp(quick.key)}
                />
            )}
            <button
                ref={buttonRef}
                type="button"
                aria-expanded={open}
                aria-controls={panelId}
                aria-label={quick ? 'Open in another app' : undefined}
                onClick={() => setOpen((o) => !o)}
                className={`inline-flex items-center gap-1 rounded-md border border-default py-1 text-xs font-semibold text-fg-muted transition-colors hover:bg-primary/5 hover:text-primary ${quick ? 'px-1.5' : 'px-2.5'}`}
            >
                {!quick && 'Open in'}
                <ChevronDownIcon className={`h-3.5 w-3.5 transition-transform ${open ? 'rotate-180' : ''}`}/>
            </button>
            {open && (
                <div
                    ref={panelRef}
                    id={panelId}
                    onClick={() => setOpen(false)}
                    className="fixed top-0 left-0 z-50 flex min-w-44 flex-col gap-1 rounded-lg border border-default bg-surface p-1.5 shadow-lg"
                >
                    {links.map(({key, label, badge}) => (
                        <AccessBadge
                            key={key}
                            href={access[key]!}
                            label={label}
                            badge={badge}
                            size="sm"
                            onOpen={() => onOpenApp(key)}
                        />
                    ))}
                </div>
            )}
        </div>
    )
}

/** Text button that copies the shareable URL for `path`, briefly flipping to
 *  a check — for referencing this location in ELN entries (#202). */
function CopyUrlButton({path}: { path: string }) {
    const [copied, copy] = useCopy()
    return (
        <button
            type="button"
            onClick={() => copy(browseUrl(path))}
            className="inline-flex shrink-0 items-center gap-1 rounded-md border border-default px-2.5 py-1 text-[11px] font-semibold text-fg-subtle transition-colors hover:bg-primary/5 hover:text-primary"
        >
            {copied ? (
                <>
                    <CheckCircleIcon className="h-3 w-3 text-success"/>
                    Copied
                </>
            ) : (
                'Copy URL'
            )}
        </button>
    )
}

/** Compact header bar for the current resource: icon + name + a meta line on the
 *  left, the full path + copy button on the right. Replaces the old detail card,
 *  matching the design's browse header. */
function ResourceBar({info, onOpenApp}: { info: BrowseV2; onOpenApp: (app: AppKey) => void }) {
    const meta = info.isdir
        ? `${info.total.toLocaleString()} item${info.total === 1 ? '' : 's'}`
        : formatSize(info.size)
    const hasLinks = APP_LINKS.some(({key}) => info.access[key])
    return (
        <Card className="mb-3.5 px-5 py-3.5">
            <div className="flex flex-wrap items-center justify-between gap-x-4 gap-y-3">
                <div className="flex min-w-0 items-center gap-2.5">
                    {info.isdir ? (
                        <FolderIcon className="h-5 w-5 shrink-0 text-fg-subtle"/>
                    ) : (
                        <FileIcon className="h-5 w-5 shrink-0 text-fg-subtle"/>
                    )}
                    <div className="min-w-0">
                        <div className="truncate font-mono text-sm font-semibold text-fg">
                            {info.name || '/'}
                        </div>
                        <div className="mt-0.5 text-xs text-fg-faint">
                            {info.project && (
                                <>
                                    in{' '}
                                    <Link
                                        to={`/projects/${encodeURIComponent(info.project)}`}
                                        className="text-fg-subtle hover:text-primary hover:underline"
                                    >
                                        {info.project}
                                    </Link>
                                    {' · '}
                                </>
                            )}
                            {meta}
                        </div>
                    </div>
                </div>
                <div className="flex min-w-0 items-center gap-2">
                    <code
                        title={info.path}
                        className="min-w-0 truncate font-mono text-[11px] text-fg-subtle"
                    >
                        {info.path}
                    </code>
                    <CopyUrlButton path={info.path}/>
                </div>
            </div>
            {hasLinks && (
                <div className="mt-3 flex flex-wrap items-center gap-1.5 border-t border-default/60 pt-3">
                    <AccessBadges access={info.access} onOpenApp={onOpenApp}/>
                </div>
            )}
        </Card>
    )
}

/** The "Access this location" card: SMB / rsync / sftp connection strings as
 *  copy fields. Renders only the commands the server returned. */
function AccessCommands({access}: { access: BrowseAccess }) {
    const fields: { label: string; value: string; hint?: string }[] = []
    if (access.smb_windows)
        fields.push({label: 'SMB (Windows)', value: access.smb_windows})
    if (access.smb_unix)
        fields.push({label: 'SMB (Linux / macOS)', value: access.smb_unix})
    if (access.rsync)
        fields.push({
            label: 'rsync',
            value: access.rsync,
            hint: 'push to er-c-data with optimized settings',
        })
    if (access.sftp) fields.push({label: 'SFTP', value: access.sftp})
    if (fields.length === 0) return null
    return (
        <Card className="px-5 py-5">
            <div className="mb-3.5 text-[13px] font-bold text-fg-muted">
                Access this location
            </div>
            <div className="space-y-3">
                {fields.map((f) => (
                    <CopyField key={f.label} label={f.label} value={f.value} hint={f.hint}/>
                ))}
            </div>
        </Card>
    )
}

/** File/folder breadcrumb for a path, prefixed with a "Projects" crumb back to
 *  the app. The trailing crumb is the current location — shown as mono text,
 *  not a link. Also the way back up from a path that could not be opened.
 *  A crumb too long for the line wraps within itself, so a name without
 *  spaces cannot run off the page. */
function Breadcrumb({path}: { path: string }) {
    const pyramid = pathPyramid(path)
    return (
        <nav className="mb-3.5 flex flex-wrap items-center gap-x-1.5 gap-y-1 text-[13px]">
            <Link to="/projects" className="rounded px-0.5 text-primary hover:underline">
                Projects
            </Link>
            {pyramid.map(([full, tail], i) => {
                const isLast = i === pyramid.length - 1
                return (
                    <Fragment key={full}>
                        <span className="text-fg-faint">›</span>
                        {isLast ? (
                            <span className="px-0.5 font-mono text-xs font-medium text-fg-muted wrap-anywhere">
                                {tail}
                            </span>
                        ) : (
                            <Link
                                to={browsePath(full)}
                                className="rounded px-0.5 text-primary wrap-anywhere hover:underline"
                            >
                                {tail}
                            </Link>
                        )}
                    </Fragment>
                )
            })}
        </nav>
    )
}

const thClass =
    'px-5 py-2.5 text-left text-[11px] font-bold uppercase tracking-wide text-fg-faint'

const rowClass =
    'border-b border-default/60 transition-colors last:border-0 hover:bg-surface-muted/60'

function EntryRow({
                      entry,
                      lastApp,
                      onOpenApp,
                  }: {
    entry: BrowseEntry
    lastApp: AppKey | null
    onOpenApp: (app: AppKey) => void
}) {
    return (
        <tr className={rowClass}>
            <td className="px-5 py-3">
                <div className="flex items-center gap-2">
                    {entry.isdir ? (
                        <FolderIcon className="h-4 w-4 shrink-0 text-fg-faint"/>
                    ) : (
                        <FileIcon className="h-4 w-4 shrink-0 text-fg-faint"/>
                    )}
                    <Link
                        to={browsePath(entry.path)}
                        className="min-w-0 font-mono text-sm text-fg wrap-anywhere hover:text-primary"
                    >
                        {entry.name}
                    </Link>
                    <CopyButton value={browseUrl(entry.path)} label="browse URL"/>
                </div>
            </td>
            <td className="whitespace-nowrap px-5 py-3 text-right font-mono text-xs text-fg-subtle">
                {formatSize(entry.size)}
            </td>
            <td className="whitespace-nowrap px-5 py-3 text-[13px] text-fg-muted">
                {formatModified(entry.mtime)}
            </td>
            <td className="px-5 py-3">
                <OpenInMenu access={entry.access} lastApp={lastApp} onOpenApp={onOpenApp}/>
            </td>
        </tr>
    )
}

/** The `..` row: the API leaves it to the client, since it follows from the path. */
function ParentRow({parent}: { parent: string }) {
    return (
        <tr className={rowClass}>
            <td className="px-5 py-3">
                <div className="flex items-center gap-2">
                    <FolderIcon className="h-4 w-4 shrink-0 text-fg-faint"/>
                    <Link
                        to={browsePath(parent)}
                        className="truncate font-mono text-sm text-fg hover:text-primary"
                    >
                        ..
                    </Link>
                </div>
            </td>
            <td className="px-5 py-3 text-right font-mono text-xs text-fg-subtle">—</td>
            <td className="px-5 py-3 text-[13px] text-fg-muted">—</td>
            <td className="px-5 py-3"/>
        </tr>
    )
}

/**
 * A folder's entries: the first page from the loader, and the pages after it
 * fetched here on "Load more". Keyed by path from the parent, so opening
 * another folder starts at page one again. The rows already shown stay put
 * while the next page loads, and stay if it fails: the error shows inline and
 * the button becomes "Try again". A button rather than infinite scroll, so
 * keyboard users can reach it and nothing depends on scroll position.
 */
function FolderListing({
                           first,
                           parent,
                           lastApp,
                           onOpenApp,
                       }: {
    first: BrowseV2
    parent: string | null
    lastApp: AppKey | null
    onOpenApp: (app: AppKey) => void
}) {
    const navigate = useNavigate()
    const [more, setMore] = useState<BrowseEntry[]>([])
    const [cursor, setCursor] = useState(first.next_cursor)
    const [loading, setLoading] = useState(false)
    const [error, setError] = useState<string | null>(null)

    const entries = [...first.entries, ...more]
    const paged = first.total > first.entries.length

    async function loadMore() {
        if (!cursor || loading) return
        setLoading(true)
        setError(null)
        try {
            const page = await apiFetchV2<BrowseV2>(browseRequest(first.path, cursor))
            setMore((shown) => [...shown, ...page.entries])
            setCursor(page.next_cursor)
        } catch (err) {
            if (err instanceof Unauthorized) {
                void navigate(`/login?next=${encodeURIComponent(browsePath(first.path))}`)
                return
            }
            setError(err instanceof Error ? err.message : 'Could not load more entries.')
        } finally {
            setLoading(false)
        }
    }

    return (
        <Card className="mb-3.5 overflow-hidden">
            {/* Fixed column widths, so the longest name in the folder cannot
                widen the Name column for every row and push the other columns
                out of the card: names wrap instead. Wrapped rather than cut off,
                since files in a series often differ only at the end. */}
            <table className="w-full table-fixed border-collapse">
                <colgroup>
                    <col/>
                    <col className="w-24"/>
                    <col className="w-48"/>
                    <col className="w-56"/>
                </colgroup>
                <thead>
                <tr className="border-b border-default">
                    <th className={thClass}>Name</th>
                    <th className={`${thClass} text-right`}>Size</th>
                    <th className={thClass}>Modified</th>
                    <th className={thClass}>Open in</th>
                </tr>
                </thead>
                <tbody>
                {parent !== null && <ParentRow parent={parent}/>}
                {entries.map((e) => (
                    <EntryRow key={e.path} entry={e} lastApp={lastApp} onOpenApp={onOpenApp}/>
                ))}
                </tbody>
            </table>
            {paged && (
                // Its own band under the rows, centred, with the button wide and
                // on its own line: right-aligned next to the count it read as
                // part of the status line and was easy to miss. Outlined rather
                // than solid: a filled block looked heavy against the rows, and
                // the plain secondary border was too faint to notice.
                <div className="flex flex-col items-center gap-2.5 border-t border-default bg-surface-muted/50 px-5 py-4 text-center">
                    <span className="text-xs font-medium text-fg-muted">
                        {entries.length.toLocaleString()} of {first.total.toLocaleString()}
                    </span>
                    {cursor && (
                        <>
                            <Button
                                variant="outline"
                                onClick={loadMore}
                                disabled={loading}
                                aria-busy={loading}
                                className="min-w-[14rem]"
                            >
                                {loading && (
                                    <span
                                        aria-hidden="true"
                                        className="h-3.5 w-3.5 animate-spin rounded-full border-2 border-current border-t-transparent"
                                    />
                                )}
                                {loading ? 'Loading…' : error ? 'Try again' : 'Load more'}
                            </Button>
                            {error && (
                                <span role="alert" className="text-xs text-danger">
                                    {error}
                                </span>
                            )}
                        </>
                    )}
                </div>
            )}
        </Card>
    )
}

/** A path that could not be opened: no such path (404), or a folder this user
 *  may not read (403). The breadcrumb above it is the way back up. */
function BrowseError({status, message}: { status: number; message: string }) {
    return (
        <Card className="border-danger/40 bg-danger/5 p-8 text-center">
            <div className="text-base font-semibold text-fg">
                {status === 403 ? 'You do not have access to this folder' : 'Could not open this path'}
            </div>
            <div className="mt-1.5 text-sm text-fg-muted">{message}</div>
        </Card>
    )
}

export function Browse() {
    const data = useLoaderData() as BrowseLoaderData
    const [lastApp, rememberApp] = useLastApp()

    if (data.error) {
        return (
            <div className="mx-auto max-w-270 px-6 py-8 animate-[fadeIn_0.2s_ease-out]">
                <Breadcrumb path={data.path}/>
                <BrowseError status={data.error.status} message={data.error.message}/>
            </div>
        )
    }

    const {result} = data
    const parent = parentOf(result.path)

    return (
        <div className="mx-auto max-w-270 px-6 py-8 animate-[fadeIn_0.2s_ease-out]">
            <Breadcrumb path={result.path}/>
            <ResourceBar info={result} onOpenApp={rememberApp}/>

            {result.isdir && (result.entries.length > 0 || parent !== null) && (
                <FolderListing
                    key={result.path}
                    first={result}
                    parent={parent}
                    lastApp={lastApp}
                    onOpenApp={rememberApp}
                />
            )}

            <AccessCommands access={result.access}/>
        </div>
    )
}
