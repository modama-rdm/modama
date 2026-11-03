import {Suspense, useState, type FormEvent} from 'react'
import {
    Await,
    Link,
    Navigate,
    useAsyncError,
    useAsyncValue,
    useLoaderData,
    useNavigate,
} from 'react-router-dom'
import {Unauthorized} from '../api/errors'
import {apiFetchV2} from '../api/clientV2'
import {
    DESCRIPTION_MAX_LENGTH,
    type ProjectDetailsV2,
    type SetDescriptionRequestV2,
} from '../api/types'
import {Breadcrumbs} from '../components/Breadcrumbs'
import {Button} from '../components/ui/Button'
import {Card} from '../components/ui/Card'
import {AlertTriangleIcon} from '../components/ui/icons'
import {Textarea} from '../components/ui/Textarea'
import {useToast} from '../components/ui/toast-context'
import {descriptionPath} from '../lib/projectsUtil'
import type {EditDescriptionLoaderData} from './editDescription.loader'

function Notice({title, detail, backTo}: { title: string; detail: string; backTo: string }) {
    return (
        <Card className="border-danger/40 bg-danger/5 p-8 text-center">
            <div className="text-base font-semibold text-fg">{title}</div>
            <div className="mt-1.5 text-sm text-fg-muted">{detail}</div>
            <Link to={backTo} className="mt-4 inline-block text-sm text-primary hover:underline">
                Back to project
            </Link>
        </Card>
    )
}

/** Renders once the `details` promise resolves; owns the edit form. */
function DescriptionForm() {
    const {name, session, currentUser} = useLoaderData() as EditDescriptionLoaderData
    const project = useAsyncValue() as ProjectDetailsV2
    const navigate = useNavigate()
    const {notify} = useToast()

    const target = session === null ? null : project.sessions.find((s) => s.name === session)
    const current = (session === null ? project.description : target?.description) ?? ''
    const [text, setText] = useState(current)
    const [error, setError] = useState('')
    const [submitting, setSubmitting] = useState(false)

    const backTo = `/projects/${project.name}`
    // The rule the API applies: the owner and the members, as for sealing.
    const canEdit = currentUser === project.owner || project.members.includes(currentUser)
    const dirty = text.trim() !== current.trim()

    if (session !== null && !target) {
        return (
            <Notice
                title="No such session"
                detail={`There is no session named "${session}" in this project.`}
                backTo={backTo}
            />
        )
    }
    if (!canEdit) {
        return (
            <Notice
                title="Only the owner and the members can change descriptions"
                detail={`This project is owned by ${project.owner}.`}
                backTo={backTo}
            />
        )
    }

    async function onSubmit(e: FormEvent) {
        e.preventDefault()
        if (!dirty) {
            navigate(backTo)
            return
        }
        setError('')
        setSubmitting(true)
        try {
            // The whole text, every time: saving the same text again changes
            // nothing, so a retry after a lost answer is safe. Empty removes
            // the README.md.
            const body: SetDescriptionRequestV2 = {description: text.trim()}
            const path =
                session === null
                    ? `/projects/${encodeURIComponent(name)}/description`
                    : `/projects/${encodeURIComponent(name)}/sessions/${encodeURIComponent(session)}/description`
            await apiFetchV2(path, {method: 'PUT', body: JSON.stringify(body)})
            notify(text.trim() ? 'Description saved.' : 'Description removed.')
            navigate(backTo)
        } catch (err) {
            if (err instanceof Unauthorized) {
                navigate(`/login?next=${descriptionPath(name, session ?? undefined)}`)
                return
            }
            // The API's errors are written to be shown.
            setError(err instanceof Error ? err.message : 'Could not save the description.')
        } finally {
            setSubmitting(false)
        }
    }

    return (
        <Card className="px-10 py-9">
            <h1 className="text-[22px] font-bold text-fg">
                {session === null ? 'Project description' : 'Session description'}
            </h1>
            <p className="mt-2 mb-7 text-sm leading-relaxed text-fg-subtle">
                {session === null ? (
                    <>
                        Saved as README.md in the project folder. Markdown is rendered on
                        the project page, and the first line shows in the projects list.
                    </>
                ) : (
                    <>
                        For <span className="font-mono text-fg">{session}</span>. Saved as
                        README.md in the session folder, next to{' '}
                        <code className="font-mono">raw/</code>; the first line shows in
                        the sessions table.
                    </>
                )}{' '}
                The owner and the members can change it.
            </p>

            {error && (
                <div
                    className="mb-5 flex items-start gap-2.5 rounded-lg border border-danger/30 bg-danger/5 px-3.5 py-3">
                    <AlertTriangleIcon className="mt-0.5 h-4 w-4 shrink-0 text-danger"/>
                    <div className="text-[13px] text-danger">{error}</div>
                </div>
            )}

            <form onSubmit={onSubmit} className="flex flex-col gap-6">
                <Textarea
                    label="Description"
                    value={text}
                    onChange={(e) => setText(e.target.value)}
                    maxLength={DESCRIPTION_MAX_LENGTH}
                    rows={8}
                    help="Leave it empty to remove the description."
                    autoFocus
                />
                <div className="mt-1 flex items-center gap-3">
                    <Button type="submit" disabled={submitting || !dirty}>
                        {submitting ? 'Saving…' : 'Save Description'}
                    </Button>
                    <Button type="button" variant="secondary" onClick={() => navigate(backTo)}>
                        Cancel
                    </Button>
                </div>
            </form>
        </Card>
    )
}

function FormSkeleton() {
    const bar = 'animate-pulse rounded bg-surface-muted'
    return (
        <Card className="p-6">
            <div className={`${bar} mb-3 h-6 w-48`}/>
            <div className={`${bar} mb-5 h-32 w-full`}/>
            <div className={`${bar} h-9 w-32`}/>
        </Card>
    )
}

/** `<Await>` error boundary — bounce to login on 401, else surface the error. */
function FormError({next}: { next: string }) {
    const error = useAsyncError()
    if (error instanceof Unauthorized) {
        return <Navigate to={`/login?next=${next}`} replace/>
    }
    return (
        <Card className="border-danger/40 bg-danger/5 p-8 text-center">
            <div className="text-base font-semibold text-fg">Could not load this project</div>
            <div className="mt-1.5 text-sm text-fg-muted">
                {error instanceof Error ? error.message : 'Please try again.'}
            </div>
        </Card>
    )
}

/** Edit a project's or a session's description. */
export function EditDescription() {
    const {name, session, details} = useLoaderData() as EditDescriptionLoaderData

    return (
        <div className="mx-auto max-w-150 px-6 py-8 animate-[fadeIn_0.2s_ease-out]">
            <Breadcrumbs
                crumbs={[
                    {label: 'Projects', to: '/projects'},
                    {label: name, to: `/projects/${name}`, mono: true},
                    ...(session === null ? [] : [{label: session, mono: true}]),
                    {label: 'Edit Description'},
                ]}
            />

            <Suspense fallback={<FormSkeleton/>}>
                <Await
                    resolve={details}
                    errorElement={<FormError next={descriptionPath(name, session ?? undefined)}/>}
                >
                    <DescriptionForm/>
                </Await>
            </Suspense>
        </div>
    )
}
