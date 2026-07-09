import {useState} from 'react'
import {useNavigate, useParams} from 'react-router-dom'
import {getCurrentUser} from '../api/auth'
import {Unauthorized, apiFetch} from '../api/client'
import type {
    GenericErrorResponse,
    SealDatasetRequest,
    SealDatasetResponse,
} from '../api/types'
import {Breadcrumbs} from '../components/Breadcrumbs'
import {Button} from '../components/ui/Button'
import {Card} from '../components/ui/Card'
import {AlertTriangleIcon, LockIcon} from '../components/ui/icons'
import {useToast} from '../components/ui/toast-context'

export function SealSession() {
    const {name = '', dataset = ''} = useParams()
    const navigate = useNavigate()
    const {notify} = useToast()

    const [error, setError] = useState('')
    const [submitting, setSubmitting] = useState(false)

    const backToProject = () => navigate(`/projects/${name}`)

    async function onSeal() {
        setError('')
        setSubmitting(true)
        try {
            const body: SealDatasetRequest = {
                name: dataset,
                user: getCurrentUser() ?? '',
            }
            const res = await apiFetch<SealDatasetResponse | GenericErrorResponse>(
                `/api/project/${encodeURIComponent(name)}/dataset/seal/`,
                {method: 'POST', body: JSON.stringify(body)},
            )
            if (res.errno !== 0) {
                setError(
                    (res as GenericErrorResponse).message ||
                    'Could not seal the session.',
                )
                return
            }
            notify(`Session "${dataset}" sealed.`)
            backToProject()
        } catch (err) {
            if (err instanceof Unauthorized) {
                navigate(`/login?next=/projects/${name}/sessions/${dataset}/seal`)
                return
            }
            setError(err instanceof Error ? err.message : 'Could not seal the session.')
        } finally {
            setSubmitting(false)
        }
    }

    return (
        <div className="mx-auto max-w-140 px-6 py-8 animate-[fadeIn_0.2s_ease-out]">
            <Breadcrumbs
                crumbs={[
                    {label: 'Projects', to: '/projects'},
                    {label: name, to: `/projects/${name}`, mono: true},
                    {label: 'Seal Session'},
                ]}
            />

            <Card className="border-danger/40 px-10 py-9 shadow-[0_4px_20px_rgb(220_38_38/0.07)]">
                <div className="mb-6 flex items-start gap-4">
                    <div
                        className="flex h-13 w-13 shrink-0 items-center justify-center rounded-xl bg-danger/10">
                        <LockIcon className="h-6 w-6 text-danger"/>
                    </div>
                    <div>
                        <h1 className="text-xl font-bold text-fg">Seal this session?</h1>
                        <p className="mt-1.5 text-sm leading-relaxed text-fg-muted">
                            This permanently write-protects the{' '}
                            <code className="rounded bg-surface-muted px-1 py-0.5 font-mono text-xs">
                                raw/
                            </code>{' '}
                            folder and all its contents.
                        </p>
                    </div>
                </div>

                <div className="mb-5 rounded-xl border border-danger/30 bg-danger/5 px-4.5 py-4">
                    <div className="mb-2 flex items-center gap-1.5 text-[13px] font-bold text-danger">
                        <AlertTriangleIcon className="h-4 w-4"/>
                        This cannot be undone
                    </div>
                    <ul className="m-0 list-disc pl-4.5 text-[13px] leading-relaxed text-danger">
                        <li>
                            All files in <code className="font-mono">raw/</code> become
                            permanently read-only
                        </li>
                        <li>Nobody — including you — can modify or delete these files</li>
                        <li>
                            The session folder and files outside{' '}
                            <code className="font-mono">raw/</code> stay writable
                        </li>
                    </ul>
                </div>

                <div className="mb-6 rounded-lg border border-default bg-surface-muted/40 px-4.5 py-3.5">
                    <div
                        className="mb-1.5 text-[11px] font-bold uppercase tracking-wide text-fg-faint">
                        Session to be sealed
                    </div>
                    <div className="font-mono text-base font-bold text-fg">{dataset}</div>
                    <div className="mt-0.5 text-xs text-fg-subtle">
                        in project <span className="font-mono">{name}</span>
                    </div>
                </div>

                {error && (
                    <div
                        className="mb-5 flex items-start gap-2.5 rounded-lg border border-danger/30 bg-danger/5 px-3.5 py-3">
                        <AlertTriangleIcon className="mt-0.5 h-4 w-4 shrink-0 text-danger"/>
                        <div className="text-[13px] text-danger">{error}</div>
                    </div>
                )}

                <p className="mb-6 text-[13px] text-fg-subtle">
                    Administrators can unseal data on special request — contact IT
                    support.
                </p>

                <div className="flex items-center gap-3">
                    <Button variant="danger" onClick={onSeal} disabled={submitting}>
                        {submitting ? 'Sealing…' : 'Seal session permanently'}
                    </Button>
                    <Button type="button" variant="secondary" onClick={backToProject}>
                        Cancel
                    </Button>
                </div>
            </Card>
        </div>
    )
}
