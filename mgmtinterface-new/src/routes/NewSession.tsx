import {useState, type FormEvent} from 'react'
import {useNavigate, useParams} from 'react-router-dom'
import {getCurrentUser} from '../api/auth'
import {Unauthorized, apiFetch} from '../api/client'
import type {
    CreateDatasetRequest,
    CreateDatasetResponse,
    GenericErrorResponse,
} from '../api/types'
import {Breadcrumbs} from '../components/Breadcrumbs'
import {Button} from '../components/ui/Button'
import {Card} from '../components/ui/Card'
import {Input} from '../components/ui/Input'
import {AlertTriangleIcon, InfoIcon} from '../components/ui/icons'
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
            <Breadcrumbs
                crumbs={[
                    {label: 'Projects', to: '/projects'},
                    {label: name, to: `/projects/${name}`, mono: true},
                    {label: 'New Session'},
                ]}
            />

            <Card className="px-10 py-9">
                <h1 className="mb-5 text-[22px] font-bold text-fg">Create a session</h1>

                <div className="mb-7 rounded-xl border border-primary/25 bg-primary/5 px-4.5 py-4">
                    <div className="mb-1.5 flex items-center gap-1.5 text-[13px] font-bold text-primary">
                        <InfoIcon className="h-4 w-4"/>
                        What is a session?
                    </div>
                    <div className="text-[13px] leading-relaxed text-fg-muted">
                        A session is a folder within this project for one measurement run
                        or experiment. Once data collection is complete, you can{' '}
                        <strong>seal</strong> it to permanently protect the raw data from
                        accidental changes.
                    </div>
                </div>

                {error && (
                    <div
                        className="mb-5 flex items-start gap-2.5 rounded-lg border border-danger/30 bg-danger/5 px-3.5 py-3">
                        <AlertTriangleIcon className="mt-0.5 h-4 w-4 shrink-0 text-danger"/>
                        <div className="text-[13px] text-danger">{error}</div>
                    </div>
                )}

                <form onSubmit={onSubmit} className="flex flex-col gap-6">
                    <Input
                        label="Session name"
                        value={session}
                        onChange={(e) => setSession(e.target.value)}
                        className="font-mono"
                        help={
                            <>
                                Pre-filled with today&#39;s date. Customize as needed —
                                e.g.{' '}
                                <code
                                    className="rounded bg-surface-muted px-1 py-0.5 font-mono text-[11px]">
                                    {sessionNamePrefix()}alignment_run
                                </code>
                            </>
                        }
                        autoFocus
                    />
                    <div className="mt-1 flex items-center gap-3">
                        <Button type="submit" disabled={submitting}>
                            {submitting ? 'Creating…' : 'Create Session'}
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
