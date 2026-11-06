import {Suspense, useState} from 'react'
import {
    Await,
    Link,
    Navigate,
    useAsyncError,
    useAsyncValue,
    useLoaderData,
    useNavigate,
} from 'react-router-dom'
import {ApiError, Unauthorized} from '../api/errors'
import {apiFetchV2, apiRequestV2} from '../api/clientV2'
import type {CreateIngestShareRequestV2, IngestShareV2, ProjectDetailsV2} from '../api/types'
import {Breadcrumbs} from '../components/Breadcrumbs'
import {CopyField} from '../components/CopyField'
import {Button} from '../components/ui/Button'
import {Card} from '../components/ui/Card'
import {
    AlertTriangleIcon,
    ArrowLeftIcon,
    InfoIcon,
    LockIcon,
    UploadIcon,
} from '../components/ui/icons'
import {useToast} from '../components/ui/toast-context'
import {findDatasetShare} from '../lib/projectsUtil'
import {shareAccess} from '../lib/shares'
import type {IngestShareLoaderData} from './ingestShare.loader'

function ErrorBanner({message}: { message: string }) {
    return (
        <div className="mb-5 flex items-start gap-2.5 rounded-lg border border-danger/30 bg-danger/5 px-3.5 py-3">
            <AlertTriangleIcon className="mt-0.5 h-4 w-4 shrink-0 text-danger"/>
            <div className="text-[13px] text-danger">{message}</div>
        </div>
    )
}

/** The connection details + Close button shown while a share is active. */
function ActiveShare({
                         share,
                         dataset,
                         onClose,
                         closing,
                     }: {
    share: IngestShareV2
    dataset: string
    onClose: () => void
    closing: boolean
}) {
    const a = shareAccess(share)
    const [tab, setTab] = useState<'windows' | 'linux'>('windows')
    const tabBase = '-mb-0.5 border-b-2 px-5 py-2 text-sm font-semibold transition-colors'
    const tabClass = (active: boolean) =>
        `${tabBase} ${
            active
                ? 'border-primary text-primary'
                : 'border-transparent text-fg-faint hover:text-fg-subtle'
        }`
    return (
        <>
            {/* Status card */}
            <Card className="mb-4 px-7 py-6">
                <div className="flex flex-wrap items-start justify-between gap-5">
                    <div>
                        <div className="mb-2 flex flex-wrap items-center gap-2.5">
                            <h1 className="text-xl font-bold text-fg">
                                Ingest share is live
                            </h1>
                            <span
                                className="inline-flex items-center gap-1.5 rounded-full bg-success/10 px-2.5 py-0.5 text-xs font-bold text-success">
                                <span className="h-1.5 w-1.5 rounded-full bg-success"/>
                                Active
                            </span>
                        </div>
                        <p className="max-w-110 text-[13px] leading-relaxed text-fg-subtle">
                            Map this ingest share on an instrument computer to upload data
                            directly into session{' '}
                            <strong className="font-mono text-xs text-fg">{dataset}</strong>{' '}
                            with your identity and permissions.
                        </p>
                    </div>
                    <Button
                        variant="dangerOutline"
                        onClick={onClose}
                        disabled={closing}
                        className="shrink-0 whitespace-nowrap"
                    >
                        {closing ? 'Stopping…' : 'Stop ingest'}
                    </Button>
                </div>
            </Card>

            {/* Credentials card */}
            <Card className="mb-4 px-7 py-6">
                <h2 className="mb-4 text-[15px] font-bold text-fg">
                    Connection credentials
                </h2>
                <div className="flex flex-col gap-3.5">
                    <CopyField label="Windows address" value={a.winAddress}/>
                    <div className="grid gap-3.5 sm:grid-cols-2">
                        <CopyField label="Username" value={a.winUsername}/>
                        <CopyField label="Password" value={a.password}/>
                    </div>
                </div>
            </Card>

            {/* Mount commands card */}
            <Card className="px-7 py-6">
                <h2 className="mb-4 text-[15px] font-bold text-fg">Mount commands</h2>
                <div className="mb-5 flex border-b-2 border-default">
                    <button
                        type="button"
                        onClick={() => setTab('windows')}
                        className={tabClass(tab === 'windows')}
                    >
                        Windows
                    </button>
                    <button
                        type="button"
                        onClick={() => setTab('linux')}
                        className={tabClass(tab === 'linux')}
                    >
                        Linux / macOS
                    </button>
                </div>
                {tab === 'windows' ? (
                    <div className="flex flex-col gap-3.5">
                        <CopyField label="Map as network drive Z:" value={a.winMap}/>
                        <div className="grid gap-3.5 sm:grid-cols-2">
                            <CopyField
                                label="Unmap drive Z:"
                                value={a.winUnmap}
                                hint="run when finished"
                            />
                            <CopyField
                                label="Unmap UNC path"
                                value={a.winUnmapExplorer}
                            />
                        </div>
                    </div>
                ) : (
                    <div className="flex flex-col gap-3.5">
                        <div
                            className="flex items-start gap-2.5 rounded-lg border border-primary/25 bg-primary/5 px-4 py-3 text-[13px] leading-relaxed text-fg-muted">
                            <InfoIcon className="mt-0.5 h-4 w-4 shrink-0 text-primary"/>
                            <span>
                                Open the SMB URL in your file manager (e.g. Nautilus,
                                macOS Finder). Enter the password when prompted — same as
                                the Windows password above.
                            </span>
                        </div>
                        <CopyField
                            label="SMB URL (file manager / Finder)"
                            value={a.unixAddress}
                        />
                        <CopyField
                            label="Password (same as Windows)"
                            value={a.password}
                        />
                    </div>
                )}
            </Card>
        </>
    )
}

/** The "no share yet" state with the create call to action. */
function CreateShare({
                         dataset,
                         sealed,
                         onCreate,
                         creating,
                     }: {
    dataset: string
    sealed: boolean
    onCreate: () => void
    creating: boolean
}) {
    return (
        <Card className="px-7 py-6">
            <h1 className="mb-2 text-xl font-bold text-fg">Ingest share</h1>
            <p className="text-sm leading-relaxed text-fg-muted">
                Creating an ingest share opens a temporary Windows network share
                (SMB/CIFS) with one-time credentials for{' '}
                <span className="font-mono text-fg">{dataset}</span>. Map it on an
                instrument computer to write data straight into this
                session with the correct ownership — no stored credentials, no system
                account.
            </p>
            {sealed && (
                <div
                    className="mt-4 flex items-start gap-2.5 rounded-lg border border-warning/40 bg-warning/5 px-3.5 py-3">
                    <LockIcon className="mt-0.5 h-4 w-4 shrink-0 text-warning"/>
                    <div className="text-[13px] text-fg-muted">
                        This session is sealed. Its{' '}
                        <code className="font-mono">raw</code> data is immutable; new
                        files can still be written elsewhere in the session.
                    </div>
                </div>
            )}
            <div className="mt-5">
                <Button onClick={onCreate} disabled={creating}>
                    <UploadIcon className="h-4 w-4"/>
                    {creating ? 'Creating…' : 'Create ingest share'}
                </Button>
            </div>
        </Card>
    )
}

/** Renders once the `details` promise resolves; owns the share state. */
function IngestPanel() {
    const {name, dataset, currentUser, shares} =
        useLoaderData() as IngestShareLoaderData
    const project = useAsyncValue() as ProjectDetailsV2
    const navigate = useNavigate()
    const {notify} = useToast()

    const ds = project.sessions.find((d) => d.name === dataset)
    // The share this user has open for the session, if any. Shares name their
    // project and session, so this is a field match, not a path comparison.
    const [share, setShare] = useState<IngestShareV2 | null>(() =>
        findDatasetShare(shares, project.name, dataset),
    )
    const [error, setError] = useState('')
    /** True when the last create answered with a share that was already open. */
    const [alreadyOpen, setAlreadyOpen] = useState(false)
    const [busy, setBusy] = useState(false)

    const backTo = `/projects/${project.name}`
    const next = `/projects/${project.name}/sessions/${dataset}/ingest`
    const canManage =
        currentUser === project.owner || project.members.includes(currentUser)

    if (!ds) {
        return (
            <Card className="border-danger/40 bg-danger/5 p-8 text-center">
                <div className="text-base font-semibold text-fg">Session not found</div>
                <div className="mt-1.5 text-sm text-fg-muted">
                    No session named{' '}
                    <span className="font-mono">{dataset}</span> in this project.
                </div>
                <Link
                    to={backTo}
                    className="mt-4 inline-block text-sm text-primary hover:underline"
                >
                    Back to project
                </Link>
            </Card>
        )
    }

    if (!canManage) {
        return (
            <Card className="border-danger/40 bg-danger/5 p-8 text-center">
                <div className="text-base font-semibold text-fg">
                    Only project members can manage ingest shares
                </div>
                <div className="mt-1.5 text-sm text-fg-muted">
                    This project is owned by {project.owner}.
                </div>
                <Link
                    to={backTo}
                    className="mt-4 inline-block text-sm text-primary hover:underline"
                >
                    Back to project
                </Link>
            </Card>
        )
    }

    async function onCreate() {
        setError('')
        setAlreadyOpen(false)
        setBusy(true)
        try {
            const body: CreateIngestShareRequestV2 = {project: name, session: dataset}
            const {status, body: created} = await apiRequestV2<IngestShareV2>(
                '/ingest-shares',
                {method: 'POST', body: JSON.stringify(body)},
            )
            setShare(created)
            // Idempotent: 200 means this user already had a share open for the
            // session, which the list we loaded had not shown yet. The details
            // are the right thing to show either way; a "Done!" toast is not.
            if (status === 200) {
                setAlreadyOpen(true)
                return
            }
            notify(`Ingest share for "${dataset}" created.`)
        } catch (err) {
            if (err instanceof Unauthorized) {
                navigate(`/login?next=${next}`)
                return
            }
            // The new API's errors are written to be shown.
            setError(
                err instanceof ApiError
                    ? err.message
                    : 'Could not create the ingest share. Please try again.',
            )
        } finally {
            setBusy(false)
        }
    }

    async function onClose() {
        if (!share) return
        setError('')
        setAlreadyOpen(false)
        setBusy(true)
        try {
            await apiFetchV2<void>(
                `/ingest-shares/${encodeURIComponent(share.share_id)}`,
                {method: 'DELETE'},
            )
            setShare(null)
            notify(`Ingest share for "${dataset}" stopped.`)
        } catch (err) {
            if (err instanceof Unauthorized) {
                navigate(`/login?next=${next}`)
                return
            }
            if (err instanceof ApiError && err.status === 404) {
                // Gone already, closed from another tab or by its owner on
                // another machine. That is the state the user wanted.
                setShare(null)
                notify(`Ingest share for "${dataset}" was already stopped.`)
                return
            }
            setError(
                err instanceof ApiError
                    ? err.message
                    : 'Could not stop the ingest share. Please try again.',
            )
        } finally {
            setBusy(false)
        }
    }

    return (
        <>
            {error && <ErrorBanner message={error}/>}
            {alreadyOpen && (
                <div
                    role="status"
                    className="mb-5 flex items-start gap-2.5 rounded-lg border border-primary/25 bg-primary/5 px-3.5 py-3">
                    <InfoIcon className="mt-0.5 h-4 w-4 shrink-0 text-primary"/>
                    <div className="text-[13px] leading-relaxed text-fg-muted">
                        You already have an ingest share for this session. These are
                        its details; no second ingest share was created.
                    </div>
                </div>
            )}
            {share ? (
                <ActiveShare
                    share={share}
                    dataset={dataset}
                    onClose={onClose}
                    closing={busy}
                />
            ) : (
                <CreateShare
                    dataset={dataset}
                    sealed={ds.immutable}
                    onCreate={onCreate}
                    creating={busy}
                />
            )}

            <div
                className="mt-6 rounded-lg border border-default/60 bg-surface-muted/30 px-4 py-3 text-[12px] leading-relaxed text-fg-subtle">
                Windows allows only one set of credentials per host, so all of your
                ingest shares reuse the same credentials and can be mapped in parallel;
                ingest shares of a different user cannot be mapped at the same time and
                must be disconnected first. List the mapped shares with{' '}
                <code className="font-mono">net use</code> and disconnect one with{' '}
                <code className="font-mono">net use &lt;drive&gt; /delete</code>.
            </div>
        </>
    )
}

function PanelSkeleton() {
    const bar = 'animate-pulse rounded bg-surface-muted'
    return (
        <>
            <div className={`${bar} mb-5 h-14 w-full`}/>
            <Card className="p-6">
                <div className={`${bar} mb-4 h-4 w-40`}/>
                {Array.from({length: 3}).map((_, i) => (
                    <div key={i} className={`${bar} mb-4 h-11 w-full last:mb-0`}/>
                ))}
            </Card>
        </>
    )
}

/** `<Await>` error boundary — bounce to login on 401, else surface the error. */
function PanelError({next}: { next: string }) {
    const error = useAsyncError()
    if (error instanceof Unauthorized) {
        return <Navigate to={`/login?next=${next}`} replace/>
    }
    return (
        <Card className="border-danger/40 bg-danger/5 p-8 text-center">
            <div className="text-base font-semibold text-fg">
                Could not load this session
            </div>
            <div className="mt-1.5 text-sm text-fg-muted">
                {error instanceof Error ? error.message : 'Please try again.'}
            </div>
        </Card>
    )
}

export function IngestShare() {
    const {name, dataset, details} = useLoaderData() as IngestShareLoaderData
    const next = `/projects/${name}/sessions/${dataset}/ingest`

    return (
        <div className="mx-auto max-w-200 px-6 py-8 animate-[fadeIn_0.2s_ease-out]">
            <Breadcrumbs
                className="mb-5"
                crumbs={[
                    {label: 'Projects', to: '/projects'},
                    {label: name, to: `/projects/${name}`, mono: true},
                    {label: `Ingest Share — ${dataset}`},
                ]}
            />

            <Suspense fallback={<PanelSkeleton/>}>
                <Await resolve={details} errorElement={<PanelError next={next}/>}>
                    <IngestPanel/>
                </Await>
            </Suspense>

            <div className="mt-5 text-center">
                <Link
                    to={`/projects/${name}`}
                    className="inline-flex items-center gap-1.5 text-[13px] font-medium text-fg-subtle hover:text-fg-muted"
                >
                    <ArrowLeftIcon className="h-3.5 w-3.5"/>
                    Back to project
                </Link>
            </div>
        </div>
    )
}
