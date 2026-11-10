import {useState, type FormEvent} from 'react'
import {Link, useNavigate, useParams} from 'react-router-dom'
import {ApiError, Unauthorized} from '../api/errors'
import {apiRequestV2} from '../api/clientV2'
import {DESCRIPTION_MAX_LENGTH, type CreateSessionRequestV2, type SessionV2} from '../api/types'
import {Breadcrumbs} from '../components/Breadcrumbs'
import {Button} from '../components/ui/Button'
import {Card} from '../components/ui/Card'
import {Input} from '../components/ui/Input'
import {Textarea} from '../components/ui/Textarea'
import {AlertTriangleIcon, InfoIcon} from '../components/ui/icons'
import {useToast} from '../components/ui/toast-context'
import {sessionNamePrefix} from '../lib/format'
import {descriptionPath} from '../lib/projectsUtil'
import type {UnsavedDescription} from './editDescription.loader'

export function NewSession() {
    const {name = ''} = useParams()
    const navigate = useNavigate()
    const {notify} = useToast()

    const [session, setSession] = useState(sessionNamePrefix)
    const [description, setDescription] = useState('')
    const [error, setError] = useState('')
    /** Name of a session that already exists, after trying to create it again. */
    const [existing, setExisting] = useState<string | null>(null)
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
        if (trimmed.toLowerCase() === 'readme.md') {
            // The API refuses it too: the project's description lives there.
            setError('A session cannot be named README.md: that is where the project keeps its description.')
            return
        }
        setError('')
        setExisting(null)
        setSubmitting(true)
        try {
            // The signed-in session is the owner; the new API takes no `owner`.
            const body: CreateSessionRequestV2 = {name: trimmed}
            // Blank is no description: leave the field out.
            if (description.trim()) body.description = description.trim()
            const {status, body: created} = await apiRequestV2<SessionV2>(
                `/projects/${encodeURIComponent(name)}/sessions`,
                {method: 'POST', body: JSON.stringify(body)},
            )
            // Creating is idempotent: 200 means the session was already there.
            // Not a success and not an error: say so on the form and let the
            // user pick another name, rather than announce "Done!".
            if (status === 200) {
                setExisting(created.name)
                return
            }
            notify(`Session "${created.name}" created.`)
            backToProject()
        } catch (err) {
            if (err instanceof Unauthorized) {
                navigate(`/login?next=/projects/${name}/sessions/new`)
                return
            }
            if (err instanceof ApiError && err.code === 'description_not_saved') {
                // As for a new project: the session is there, its README.md
                // is not. Carry the text over to its description.
                const state: UnsavedDescription = {
                    text: description.trim(),
                    error: `Session "${trimmed}" was created, but its description could not be saved. Please save it again.`,
                }
                navigate(descriptionPath(name, trimmed), {state})
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
                        A session keeps the data of one stretch of work at one instrument,
                        usually no longer than a day, in its own folder within this project.
                        Once data collection is complete, you can{' '}
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

                {existing && (
                    <div
                        role="status"
                        className="mb-5 flex items-start gap-2.5 rounded-lg border border-warning/40 bg-warning/5 px-3.5 py-3">
                        <AlertTriangleIcon className="mt-0.5 h-4 w-4 shrink-0 text-warning"/>
                        <div className="text-[13px] leading-relaxed text-fg-muted">
                            A session named{' '}
                            <span className="font-mono text-fg">{existing}</span> already
                            exists in this project. Choose another name, or{' '}
                            <Link
                                to={`/projects/${name}`}
                                className="font-medium text-primary hover:underline"
                            >
                                go back to the project
                            </Link>
                            .
                        </div>
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
                    <Textarea
                        label={
                            <>
                                Description{' '}
                                <span className="font-normal text-fg-faint">(optional)</span>
                            </>
                        }
                        value={description}
                        onChange={(e) => setDescription(e.target.value)}
                        maxLength={DESCRIPTION_MAX_LENGTH}
                        placeholder="Sample, settings, what this run is for…"
                        help={
                            <>
                                Saved as README.md in the session folder, next to{' '}
                                <code className="font-mono">raw/</code>, so it stays editable
                                after sealing. The first line shows in the sessions table.
                            </>
                        }
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
