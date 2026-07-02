import {useState, type FormEvent} from 'react'
import {Link, useLoaderData, useNavigate} from 'react-router-dom'
import {Unauthorized, apiFetch} from '../api/client'
import type {
    CreateProjectRequest,
    CreateProjectResponse,
    GenericErrorResponse,
} from '../api/types'
import {MemberPicker} from '../components/MemberPicker'
import {Button} from '../components/ui/Button'
import {Card} from '../components/ui/Card'
import {Input} from '../components/ui/Input'
import {AlertTriangleIcon, ArrowLeftIcon} from '../components/ui/icons'
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
            <Link
                to="/projects"
                className="mb-5 inline-flex items-center gap-1.5 text-sm text-fg-subtle hover:text-fg"
            >
                <ArrowLeftIcon className="h-4 w-4"/>
                Projects
            </Link>

            <h1 className="text-2xl font-bold text-fg">Create project</h1>
            <p className="mt-0.5 mb-6 text-sm text-fg-subtle">
                Projects are folders under <code className="font-mono">/adhoc</code>. You
                own it and manage its membership.
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
                        label="Project name"
                        value={name}
                        onChange={(e) => setName(e.target.value)}
                        placeholder="e.g. ghost_imaging"
                        autoFocus
                    />
                    <MemberPicker
                        allUsers={users}
                        owner={currentUser}
                        selected={members}
                        onChange={setMembers}
                    />
                    <div className="mt-1 flex items-center gap-3">
                        <Button type="submit" disabled={submitting}>
                            {submitting ? 'Creating…' : 'Create project'}
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
