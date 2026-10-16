import {useState, type FormEvent} from 'react'
import {Link, useLoaderData, useNavigate} from 'react-router-dom'
import {Unauthorized} from '../api/client'
import {apiRequestV2} from '../api/clientV2'
import type {CreateProjectRequestV2, SerializedProject} from '../api/types'
import {Breadcrumbs} from '../components/Breadcrumbs'
import {MemberPicker} from '../components/MemberPicker'
import {Button} from '../components/ui/Button'
import {Card} from '../components/ui/Card'
import {Input} from '../components/ui/Input'
import {AlertTriangleIcon} from '../components/ui/icons'
import {useToast} from '../components/ui/toast-context'
import type {NewProjectLoaderData} from './newProject.loader'

/**
 * The rule the API enforces on project names (and the permissions service
 * behind it): a Unix group name, since a project may become one. Checked here
 * too, so the form can say it in words rather than show the API's pattern.
 */
const PROJECT_NAME = /^[a-z][a-z0-9_-]*$/

function projectNameProblem(name: string): string | null {
    if (name.length < 3) return 'Project name must be at least 3 characters long.'
    if (name.length > 32) return 'Project name can be at most 32 characters long.'
    if (!PROJECT_NAME.test(name)) {
        return 'Project name can only use lowercase letters, digits, "_" and "-", and must start with a letter.'
    }
    return null
}

export function NewProject() {
    const {currentUser, users} = useLoaderData() as NewProjectLoaderData
    const navigate = useNavigate()
    const {notify} = useToast()

    const [name, setName] = useState('')
    const [members, setMembers] = useState<string[]>([])
    const [error, setError] = useState('')
    /** Name of a project the user already owns, after trying to create it again. */
    const [existing, setExisting] = useState<string | null>(null)
    const [submitting, setSubmitting] = useState(false)

    async function onSubmit(e: FormEvent) {
        e.preventDefault()
        const trimmed = name.trim()
        if (!trimmed) {
            setError('Please enter a project name.')
            return
        }
        const problem = projectNameProblem(trimmed)
        if (problem) {
            setError(problem)
            return
        }
        setError('')
        setExisting(null)
        setSubmitting(true)
        try {
            // The signed-in session is the owner; the new API takes no `owner`.
            const body: CreateProjectRequestV2 = {name: trimmed, members}
            const {status, body: project} = await apiRequestV2<SerializedProject>(
                '/projects',
                {method: 'POST', body: JSON.stringify(body)},
            )
            // Creating is idempotent: 200 means a project of that name already
            // belongs to this user. Not a success and not an error: the user
            // may have meant a new name, so say so here and let them choose,
            // rather than announce "Done!" and jump to the old project.
            if (status === 200) {
                setExisting(project.name)
                return
            }
            notify(`Project "${project.name}" created.`)
            navigate(`/projects/${project.name}`)
        } catch (err) {
            if (err instanceof Unauthorized) {
                navigate('/login?next=/projects/new')
                return
            }
            setError(err instanceof Error ? err.message : 'Could not create the project.')
        } finally {
            setSubmitting(false)
        }
    }

    return (
        <div className="mx-auto max-w-150 px-6 py-8 animate-[fadeIn_0.2s_ease-out]">
            <Breadcrumbs
                crumbs={[
                    {label: 'Projects', to: '/projects'},
                    {label: 'New Project'},
                ]}
            />

            <Card className="px-10 py-9">
                <h1 className="text-[22px] font-bold text-fg">Create a new project</h1>
                <p className="mt-2 mb-8 text-sm leading-relaxed text-fg-subtle">
                    A project is a shared folder for your experiment data. All members
                    can contribute files; you as owner manage membership.
                </p>

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
                            You already own a project named{' '}
                            <span className="font-mono text-fg">{existing}</span>. Choose
                            another name, or{' '}
                            <Link
                                to={`/projects/${existing}`}
                                className="font-medium text-primary hover:underline"
                            >
                                open the existing project
                            </Link>
                            .
                        </div>
                    </div>
                )}

                <form onSubmit={onSubmit} className="flex flex-col gap-6">
                    <Input
                        label={
                            <>
                                Project name <span className="text-danger">*</span>
                            </>
                        }
                        value={name}
                        onChange={(e) => setName(e.target.value)}
                        placeholder="e.g. ghost_imaging_2025"
                        className="font-mono"
                        help="Lowercase letters, digits, underscores and dashes, 3 to 32 characters. Will become a folder name."
                        autoFocus
                    />
                    <MemberPicker
                        allUsers={users}
                        owner={currentUser}
                        selected={members}
                        onChange={setMembers}
                        optional
                    />
                    <div className="mt-1 flex items-center gap-3">
                        <Button type="submit" disabled={submitting}>
                            {submitting ? 'Creating…' : 'Create Project'}
                        </Button>
                        <Button
                            type="button"
                            variant="secondary"
                            onClick={() => navigate('/projects')}
                        >
                            Cancel
                        </Button>
                    </div>
                </form>
            </Card>
        </div>
    )
}
