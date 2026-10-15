import {screen} from '@testing-library/react'
import {beforeEach, describe, expect, it} from 'vitest'
import {installApiMock, type ApiRecorder} from '../api-mock'
import {renderApp} from '../render'
import {baseWorld} from '../world'

/**
 * #169 journey 1/6 — login. Gates every other journey (they start via
 * `signIn()` and skip this), so it's the one place the actual `POST
 * /mgmt-api/auth/login` -> stored credential -> redirect chain gets exercised.
 *
 * Sign-in moved to the FastAPI endpoint with #172: it answers with real status
 * codes and sets an httpOnly session cookie the test environment never sees,
 * which is the point — the app cannot read it either.
 */
describe('journey: login', () => {
    let recorder: ApiRecorder

    beforeEach(() => {
        recorder = installApiMock([
            ...baseWorld(),
            {
                method: 'POST',
                path: '/mgmt-api/auth/login',
                respond: ({body}) => {
                    const {username, password, persistent} = body as {
                        username: string
                        password: string
                        persistent?: boolean
                    }
                    if (username === 'user1' && password === 'correct-horse') {
                        return {body: {username, persistent: persistent === true}}
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
        // "Keep me logged in" is off unless the user ticks it.
        expect(screen.getByLabelText('Keep me logged in')).not.toBeChecked()
        await user.click(screen.getByRole('button', {name: /sign in/i}))

        expect(await screen.findByRole('heading', {name: 'Projects'})).toBeInTheDocument()
        const [call] = recorder.matching('POST', '/mgmt-api/auth/login')
        expect(call.body).toEqual({username: 'user1', password: 'correct-horse', persistent: false})
    })

    it('sends the "keep me logged in" choice with the credentials', async () => {
        const {user} = renderApp({route: '/login'})

        await user.type(screen.getByLabelText('Username'), 'user1')
        await user.type(screen.getByLabelText('Password'), 'correct-horse')
        await user.click(screen.getByLabelText('Keep me logged in'))
        await user.click(screen.getByRole('button', {name: /sign in/i}))

        expect(await screen.findByRole('heading', {name: 'Projects'})).toBeInTheDocument()
        const [call] = recorder.matching('POST', '/mgmt-api/auth/login')
        expect(call.body).toEqual({username: 'user1', password: 'correct-horse', persistent: true})
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
