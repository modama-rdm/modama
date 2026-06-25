import {useState, type FormEvent} from 'react'
import {Link, useNavigate, useParams} from 'react-router-dom'
import {getCurrentUser} from '../api/auth'
import {Unauthorized, apiFetch} from '../api/client'
import type {
    CreateDatasetRequest,
    CreateDatasetResponse,
    GenericErrorResponse,
} from '../api/types'
import {Button} from '../components/ui/Button'
import {Card} from '../components/ui/Card'
import {Input} from '../components/ui/Input'
import {AlertTriangleIcon, ArrowLeftIcon} from '../components/ui/icons'
import {useToast} from '../components/ui/toast-context'
import {sessionNamePrefix} from '../lib/format'

export function NewSession() {
    const {name = ''} = useParams()
    const navigate = useNavigate()
    const {notify} = useToast()

    const [session, setSession] = useState(sessionNamePrefix)
    const [error, setError] = useState('')
    const [submitting, setSubmitting] = useState(false)

    const backToProject = () => navigate(`/projects/${name}`)

    async function onSubmit(e: FormEvent) {
        e.preventDefault()
        const trimmed = session.trim()
        if (!trimmed) {
            setError('Please enter a session name.')
            return
        }
        if (trimmed.includes('/')) {
            setError('Session name cannot contain a "/".')
            return
        }
        setError('')
        setSubmitting(true)
        try {
            const body: CreateDatasetRequest = {
                name: trimmed,
                owner: getCurrentUser() ?? '',
            }
            const res = await apiFetch<CreateDatasetResponse | GenericErrorResponse>(
                `/api/project/${encodeURIComponent(name)}/dataset/`,
                {method: 'POST', body: JSON.stringify(body)},
            )
            if (res.errno !== 0) {
                setError(
                    (res as GenericErrorResponse).message ||
                    'Could not create the session.',
                )
                return
            }
            notify(`Session "${trimmed}" created.`)
            backToProject()
        } catch (err) {
            if (err instanceof Unauthorized) {
                navigate(`/login?next=/projects/${name}/sessions/new`)
                return
            }
            setError(err instanceof Error ? err.message : 'Could not create the session.')
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

            <h1 className="text-2xl font-bold text-fg">Create session</h1>
            <p className="mt-0.5 mb-6 text-sm text-fg-subtle">
                A session is a top-level folder in{' '}
                <span className="font-mono text-fg-muted">{name}</span> with a{' '}
                <code className="font-mono">raw</code> subfolder. It can be mapped as a
                network drive on a microscope and sealed once data collection is done.
            </p>

            {error && (
                <div
                    className="mb-5 flex items-start gap-2.5 rounded-lg border border-danger/30 bg-danger/5 px-3.5 py-3">
                    <AlertTriangleIcon className="mt-0.5 h-4 w-4 shrink-0 text-danger"/>
                    <div className="text-[13px] text-danger">{error}</div>
                </div>
            )}

            <Card className="p-6">
                <form onSubmit={onSubmit} className="flex flex-col gap-5">
                    <Input
                        label="Session name"
                        value={session}
                        onChange={(e) => setSession(e.target.value)}
                        placeholder="e.g. 2026-06-25"
                        autoFocus
                    />
                    <div className="mt-1 flex items-center gap-3">
                        <Button type="submit" disabled={submitting}>
                            {submitting ? 'Creating…' : 'Create session'}
                        </Button>
                        <Button type="button" variant="secondary" onClick={backToProject}>
                            Cancel
                        </Button>
                    </div>
                </form>
            </Card>
        </div>
    )
}
