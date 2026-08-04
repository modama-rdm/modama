import {screen} from '@testing-library/react'
import {beforeEach, describe, expect, it} from 'vitest'
import {installApiMock} from '../api-mock'
import {renderApp} from '../render'
import {baseWorld} from '../world'

/**
 * #169 journey 1/6 — login. Gates every other journey (they start via
 * `signIn()` and skip this), so it's the one place the actual `POST
 * /api/v2/auth/login` -> stored credential -> redirect chain gets exercised.
 *
 * Sign-in moved to the FastAPI endpoint with #172: it answers with real status
 * codes and sets an httpOnly session cookie the test environment never sees,
 * which is the point — the app cannot read it either.
 */
describe('journey: login', () => {
    beforeEach(() => {
        installApiMock([
            ...baseWorld(),
            {
                method: 'POST',
                path: '/api/v2/auth/login',
                respond: ({body}) => {
                    const {username, password} = body as {username: string; password: string}
                    if (username === 'user1' && password === 'correct-horse') {
                        return {body: {username}}
                    }
                    return {
                        status: 401,
                        body: {
                            code: 'invalid_credentials',
                            message: 'Wrong username or password.',
                        },
                    }
                },
            },
        ])
    })

    it('signs in with valid credentials and lands on Projects', async () => {
        const {user} = renderApp({route: '/login'})

        await user.type(screen.getByLabelText('Username'), 'user1')
        await user.type(screen.getByLabelText('Password'), 'correct-horse')
        await user.click(screen.getByRole('button', {name: /sign in/i}))

        expect(await screen.findByRole('heading', {name: 'Projects'})).toBeInTheDocument()
    })

    it('shows an error and stays on the login form for bad credentials', async () => {
        const {user} = renderApp({route: '/login'})

        await user.type(screen.getByLabelText('Username'), 'user1')
        await user.type(screen.getByLabelText('Password'), 'wrong-password')
        await user.click(screen.getByRole('button', {name: /sign in/i}))

        expect(await screen.findByText('Sign-in failed')).toBeInTheDocument()
        expect(screen.getByLabelText('Username')).toBeInTheDocument()
    })

    it('preserves the ?next= redirect target through login', async () => {
        const {user} = renderApp({route: '/login?next=%2Fprojects%2Fnew'})

        await user.type(screen.getByLabelText('Username'), 'user1')
        await user.type(screen.getByLabelText('Password'), 'correct-horse')
        await user.click(screen.getByRole('button', {name: /sign in/i}))

        expect(await screen.findByRole('heading', {name: /create a new project/i})).toBeInTheDocument()
    })
})
