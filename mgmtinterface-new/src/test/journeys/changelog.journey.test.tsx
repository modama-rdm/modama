import {screen} from '@testing-library/react'
import {beforeEach, describe, expect, it} from 'vitest'
import {CHANGELOG} from '../../lib/changelog'
import {installApiMock} from '../api-mock'
import {renderApp, signIn} from '../render'
import {baseWorld} from '../world'

/**
 * Journey — the changelog screen, read from CHANGELOG.md. It shows each
 * release's title and bullets, with inline Markdown rendered, and leaves out
 * the notes for developers and the issue references.
 */
describe('journey: the changelog', () => {
    beforeEach(() => {
        signIn('user1')
        installApiMock(baseWorld())
    })

    it('shows the newest release first, with its title', async () => {
        renderApp({route: '/changelog'})

        const headings = await screen.findAllByRole('heading', {level: 2})
        expect(headings).toHaveLength(CHANGELOG.length)
        expect(headings[0]).toHaveTextContent(`v${__APP_VERSION__}${CHANGELOG[0].title}`)
    })

    it('renders inline code in a bullet', async () => {
        renderApp({route: '/changelog'})

        const code = await screen.findAllByText('README.md', {selector: 'code'})
        expect(code.length).toBeGreaterThan(0)
    })

    it('leaves out the Internal notes and the issue references', async () => {
        renderApp({route: '/changelog'})
        await screen.findAllByText('README.md', {selector: 'code'})

        expect(screen.queryByText(/inode change time/)).not.toBeInTheDocument()
        expect(screen.queryByText(/backend image rebuild/)).not.toBeInTheDocument()
        expect(screen.queryByText(/\(#\d+\)/)).not.toBeInTheDocument()
    })
})
