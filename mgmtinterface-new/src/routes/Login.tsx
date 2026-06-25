import {useEffect, useState} from 'react'
import {useNavigate, useSearchParams} from 'react-router-dom'
import {isAuthenticated, login} from '../api/auth'
import {Button} from '../components/ui/Button'
import {Card} from '../components/ui/Card'
import {Input} from '../components/ui/Input'
import {AlertTriangleIcon} from '../components/ui/icons'
import * as React from "react";

/**
 * Only allow same-origin relative redirect targets (mirrors the backend's
 * `is_safe_url` check on the login route's ?next param).
 */
function safeNext(next: string | null): string {
    if (next && next.startsWith('/') && !next.startsWith('//')) return next
    return '/projects'
}

export function Login() {
    const navigate = useNavigate()
    const [searchParams] = useSearchParams()
    const next = safeNext(searchParams.get('next'))

    const [username, setUsername] = useState('')
    const [password, setPassword] = useState('')
    const [error, setError] = useState('')
    const [submitting, setSubmitting] = useState(false)

    // Already signed in? Skip the form.
    useEffect(() => {
        if (isAuthenticated()) navigate(next, {replace: true})
    }, [navigate, next])

    async function onSubmit(e: React.FormEvent) {
        e.preventDefault()
        if (!username || !password) {
            setError('Please enter both username and password.')
            return
        }
        setError('')
        setSubmitting(true)
        const result = await login(username, password)
        setSubmitting(false)
        if (result.ok) {
            navigate(next, {replace: true})
        } else {
            setError(result.message)
        }
    }

    return (
        <div className="flex min-h-screen items-center justify-center bg-bg p-6">
            <div className="w-full max-w-100 animate-[fadeIn_0.3s_ease-out]">
                <Card className="px-10 py-11">
                    <div className="mb-8 text-center">
                        <img src={`${import.meta.env.BASE_URL}modama.svg`} alt="modama"
                             className="mx-auto h-9"/>
                        <div className="mt-3.5 text-lg font-bold text-fg">Data Management</div>
                        <div className="mt-1 text-[13px] text-fg-subtle">
                            ER-C Self-Service Portal
                        </div>
                    </div>

                    {error && (
                        <div
                            className="mb-5 flex items-start gap-2.5 rounded-lg border border-danger/30 bg-danger/5 px-3.5 py-3">
                            <AlertTriangleIcon className="mt-0.5 h-4 w-4 shrink-0 text-danger"/>
                            <div>
                                <div className="text-[13px] font-semibold text-danger">
                                    Sign-in failed
                                </div>
                                <div className="mt-0.5 text-xs text-danger">{error}</div>
                            </div>
                        </div>
                    )}

                    <form onSubmit={onSubmit} className="flex flex-col gap-4">
                        <Input
                            label="Username"
                            value={username}
                            onChange={(e) => setUsername(e.target.value)}
                            placeholder="e.g. barthel"
                            autoComplete="username"
                            autoFocus
                        />
                        <Input
                            label="Password"
                            type="password"
                            value={password}
                            onChange={(e) => setPassword(e.target.value)}
                            placeholder="Unix password"
                            autoComplete="current-password"
                        />
                        <Button type="submit" fullWidth disabled={submitting} className="mt-2">
                            {submitting ? 'Signing in…' : 'Sign in →'}
                        </Button>
                    </form>

                    <div
                        className="mt-6 border-t border-default pt-5 text-center text-xs leading-relaxed text-fg-faint">
                        Use your IFF/ER-C Unix credentials.
                        <br/>
                        Contact IT support if you cannot log in.
                    </div>
                </Card>
            </div>
        </div>
    )
}
