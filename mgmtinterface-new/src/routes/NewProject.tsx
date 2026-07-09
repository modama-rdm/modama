import {useState, type FormEvent} from 'react'
import {useLoaderData, useNavigate} from 'react-router-dom'
import {Unauthorized, apiFetch} from '../api/client'
import type {
    CreateProjectRequest,
    CreateProjectResponse,
    GenericErrorResponse,
} from '../api/types'
import {Breadcrumbs} from '../components/Breadcrumbs'
import {MemberPicker} from '../components/MemberPicker'
import {Button} from '../components/ui/Button'
import {Card} from '../components/ui/Card'
import {Input} from '../components/ui/Input'
import {AlertTriangleIcon} from '../components/ui/icons'
import {useToast} from '../components/ui/toast-context'
import type {NewProjectLoaderData} from './newProject.loader'

export function NewProject() {
    const {currentUser, users} = useLoaderData() as NewProjectLoaderData
    const navigate = useNavigate()
    const {notify} = useToast()

    const [name, setName] = useState('')
    const [members, setMembers] = useState<string[]>([])
    const [error, setError] = useState('')
    const [submitting, setSubmitting] = useState(false)

    async function onSubmit(e: FormEvent) {
        e.preventDefault()
        const trimmed = name.trim()
        if (!trimmed) {
            setError('Please enter a project name.')
            return
        }
        if (trimmed.includes('/')) {
            setError('Project name cannot contain a "/".')
            return
        }
        setError('')
        setSubmitting(true)
        try {
            const body: CreateProjectRequest = {
                name: trimmed,
                owner: currentUser,
                members,
            }
            const res = await apiFetch<CreateProjectResponse | GenericErrorResponse>(
                '/api/project/',
                {method: 'POST', body: JSON.stringify(body)},
            )
            // `errno` can't discriminate (it's `number` on the error shape), so
            // key off the presence of the `project` payload.
            if (!('project' in res)) {
                setError(res.message || 'Could not create the project.')
                return
            }
            notify(`Project "${res.project.name}" created.`)
            navigate(`/projects/${res.project.name}`)
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
                        help="Lowercase letters, numbers, and underscores only. Will become a folder name."
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
