import {useState} from 'react'
import {Link, useNavigate, useParams} from 'react-router-dom'
import {getCurrentUser} from '../api/auth'
import {Unauthorized, apiFetch} from '../api/client'
import type {
    GenericErrorResponse,
    SealDatasetRequest,
    SealDatasetResponse,
} from '../api/types'
import {Button} from '../components/ui/Button'
import {Card} from '../components/ui/Card'
import {AlertTriangleIcon, ArrowLeftIcon, LockIcon} from '../components/ui/icons'
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
        <div className="mx-auto max-w-150 px-6 py-8 animate-[fadeIn_0.2s_ease-out]">
            <Link
                to={`/projects/${name}`}
                className="mb-5 inline-flex items-center gap-1.5 text-sm text-fg-subtle hover:text-fg"
            >
                <ArrowLeftIcon className="h-4 w-4"/>
                <span className="font-mono">{name}</span>
            </Link>

            <h1 className="flex items-center gap-2 text-2xl font-bold text-fg">
                <LockIcon className="h-6 w-6 text-fg-muted"/>
                Seal session
            </h1>
            <p className="mt-0.5 mb-6 text-sm text-fg-subtle">
                Sealing <span className="font-mono text-fg-muted">{dataset}</span> in{' '}
                <span className="font-mono text-fg-muted">{name}</span>.
            </p>

            {error && (
                <div
                    className="mb-5 flex items-start gap-2.5 rounded-lg border border-danger/30 bg-danger/5 px-3.5 py-3">
                    <AlertTriangleIcon className="mt-0.5 h-4 w-4 shrink-0 text-danger"/>
                    <div className="text-[13px] text-danger">{error}</div>
                </div>
            )}

            <Card className="p-6">
                <div className="flex items-start gap-2.5 rounded-lg border border-warning/40 bg-warning/5 px-3.5 py-3">
                    <AlertTriangleIcon className="mt-0.5 h-4 w-4 shrink-0 text-warning"/>
                    <div className="text-[13px] leading-relaxed text-fg-muted">
                        This makes the <code className="font-mono">raw</code> subfolder and
                        all its contents <strong>immutable</strong>. The rest of the session
                        stays writable. Only an administrator can unseal it later, on
                        request.
                    </div>
                </div>
                <div className="mt-5 flex items-center gap-3">
                    <Button variant="danger" onClick={onSeal} disabled={submitting}>
                        <LockIcon className="h-4 w-4"/>
                        {submitting ? 'Sealing…' : `Seal "${dataset}"`}
                    </Button>
                    <Button type="button" variant="secondary" onClick={backToProject}>
                        Cancel
                    </Button>
                </div>
            </Card>
        </div>
    )
}
