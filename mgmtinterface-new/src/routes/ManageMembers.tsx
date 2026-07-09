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
import {Unauthorized, apiFetch} from '../api/client'
import type {
    ChangeMembersRequest,
    ChangeMembersResponse,
    GenericErrorResponse,
    ProjectDetailsResponse,
} from '../api/types'
import {Breadcrumbs} from '../components/Breadcrumbs'
import {MemberPicker} from '../components/MemberPicker'
import {Avatar} from '../components/ui/Avatar'
import {Button} from '../components/ui/Button'
import {Card} from '../components/ui/Card'
import {AlertTriangleIcon} from '../components/ui/icons'
import {useToast} from '../components/ui/toast-context'
import {memberChanges} from '../lib/projectsUtil'
import type {ManageMembersLoaderData} from './manageMembers.loader'

/**
 * Apply one add/remove batch via `PATCH …/members/`. Throws on a non-success
 * response so the caller can surface the backend message; `Unauthorized`
 * propagates untouched for the login redirect.
 */
async function changeMembers(
    name: string,
    body: ChangeMembersRequest,
): Promise<void> {
    const res = await apiFetch<ChangeMembersResponse | GenericErrorResponse>(
        `/api/project/${encodeURIComponent(name)}/members/`,
        {method: 'PATCH', body: JSON.stringify(body)},
    )
    // `errno` can't discriminate (it's `number` on the error shape), so key off
    // the presence of the `project` payload (matches the New Project pattern).
    if (!('project' in res)) {
        throw new Error(res.message || 'Could not update members.')
    }
}

/** Renders once the slow `details` promise resolves; owns the edit form. */
function MembersForm() {
    const {name, currentUser, users} = useLoaderData() as ManageMembersLoaderData
    const {project} = useAsyncValue() as ProjectDetailsResponse
    const navigate = useNavigate()
    const {notify} = useToast()

    const [selected, setSelected] = useState<string[]>(project.members)
    const [error, setError] = useState('')
    const [submitting, setSubmitting] = useState(false)

    const backTo = `/projects/${project.name}`
    const {added, removed} = memberChanges(project.members, selected)
    const dirty = added.length > 0 || removed.length > 0

    if (currentUser !== project.owner) {
        return (
            <Card className="border-danger/40 bg-danger/5 p-8 text-center">
                <div className="text-base font-semibold text-fg">
                    Only the owner can manage members
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

    async function onSubmit(e: FormEvent) {
        e.preventDefault()
        if (!dirty) {
            navigate(backTo)
            return
        }
        setError('')
        setSubmitting(true)
        // The API applies one op per call, so add + remove can't be atomic. Track
        // whether the add already committed: if a later call fails we've changed
        // server state, and the form's baseline (`project.members`, loaded once)
        // is now stale — resubmitting would re-add the already-added members.
        let partiallyApplied = false
        try {
            // Two ops mirror the legacy route: add the new members, then remove
            // the dropped ones. `user` must equal the authenticated user.
            if (added.length > 0) {
                await changeMembers(name, {op: 'add', members: added, user: currentUser})
                partiallyApplied = removed.length > 0
            }
            if (removed.length > 0) {
                await changeMembers(name, {op: 'remove', members: removed, user: currentUser})
            }
            notify(`Members of "${project.name}" updated.`)
            navigate(backTo)
        } catch (err) {
            if (err instanceof Unauthorized) {
                navigate(`/login?next=/projects/${project.name}/members`)
                return
            }
            const message = err instanceof Error ? err.message : 'Could not update members.'
            if (partiallyApplied) {
                // Some changes already landed. Bounce to the project (which reloads
                // the real member list) instead of leaving a stale form the user
                // could resubmit against an out-of-date baseline.
                notify(`Some changes were saved before an error occurred: ${message} Please re-check the members.`, 'error')
                navigate(backTo)
            } else {
                // Nothing was applied — the baseline still matches the server, so
                // it is safe to surface the error in place and let the user retry.
                setError(message)
            }
        } finally {
            setSubmitting(false)
        }
    }

    return (
        <Card className="px-10 py-9">
            <h1 className="text-[22px] font-bold text-fg">Manage members</h1>
            <p className="mt-2 mb-7 text-sm leading-relaxed text-fg-subtle">
                Members can create files and folders within this project. Only you as
                project owner can manage membership.
            </p>

            {error && (
                <div
                    className="mb-5 flex items-start gap-2.5 rounded-lg border border-danger/30 bg-danger/5 px-3.5 py-3">
                    <AlertTriangleIcon className="mt-0.5 h-4 w-4 shrink-0 text-danger"/>
                    <div className="text-[13px] text-danger">{error}</div>
                </div>
            )}

            <div className="mb-6">
                <div className="mb-2.5 text-[13px] font-semibold text-fg-muted">
                    Project owner
                </div>
                <div
                    className="inline-flex items-center gap-2.5 rounded-xl border border-success/30 bg-success/5 px-3.5 py-2.5">
                    <Avatar username={project.owner} size={32}/>
                    <span className="text-sm font-semibold text-success">
                        {project.owner}
                    </span>
                    <span
                        className="rounded bg-success/15 px-1.5 py-0.5 text-[10px] font-bold uppercase tracking-wider text-success">
                        Owner
                    </span>
                </div>
            </div>

            <form onSubmit={onSubmit} className="flex flex-col gap-6">
                <MemberPicker
                    allUsers={users}
                    owner={project.owner}
                    selected={selected}
                    onChange={setSelected}
                    helpText="Search by username to add a member; use the remove button on a chip to drop one."
                />
                <div className="mt-1 flex items-center gap-3">
                    <Button type="submit" disabled={submitting || !dirty}>
                        {submitting ? 'Saving…' : 'Save Changes'}
                    </Button>
                    <Button
                        type="button"
                        variant="secondary"
                        onClick={() => navigate(backTo)}
                    >
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
        <>
            <div className={`${bar} mb-6 h-8 w-64`}/>
            <Card className="p-6">
                <div className={`${bar} mb-3 h-4 w-20`}/>
                <div className={`${bar} mb-5 h-11 w-full`}/>
                <div className={`${bar} h-9 w-32`}/>
            </Card>
        </>
    )
}

/** `<Await>` error boundary — bounce to login on 401, else surface the error. */
function FormError({name}: { name: string }) {
    const error = useAsyncError()
    if (error instanceof Unauthorized) {
        return <Navigate to={`/login?next=/projects/${name}/members`} replace/>
    }
    return (
        <Card className="border-danger/40 bg-danger/5 p-8 text-center">
            <div className="text-base font-semibold text-fg">
                Could not load this project
            </div>
            <div className="mt-1.5 text-sm text-fg-muted">
                {error instanceof Error ? error.message : 'Please try again.'}
            </div>
        </Card>
    )
}

export function ManageMembers() {
    const {name, details} = useLoaderData() as ManageMembersLoaderData

    return (
        <div className="mx-auto max-w-150 px-6 py-8 animate-[fadeIn_0.2s_ease-out]">
            <Breadcrumbs
                crumbs={[
                    {label: 'Projects', to: '/projects'},
                    {label: name, to: `/projects/${name}`, mono: true},
                    {label: 'Manage Members'},
                ]}
            />

            <Suspense fallback={<FormSkeleton/>}>
                <Await resolve={details} errorElement={<FormError name={name}/>}>
                    <MembersForm/>
                </Await>
            </Suspense>
        </div>
    )
}
