import { Card } from '../components/ui/Card'
import { FolderIcon } from '../components/ui/icons'

/**
 * Placeholder landing for the authenticated area. Proves the protected route +
 * app shell render after login; the real Projects screen arrives in a later PR.
 */
export function Projects() {
  return (
    <div className="mx-auto max-w-[1080px] px-6 py-8 [animation:fadeIn_0.2s_ease-out]">
      <h1 className="text-2xl font-bold text-fg">Projects</h1>
      <p className="mt-1 text-sm text-fg-subtle">
        Your experimental data, organized by project and session.
      </p>
      <Card className="mt-6 p-12 text-center">
        <FolderIcon className="mx-auto h-8 w-8 text-fg-faint" />
        <div className="mt-3 text-base font-semibold text-fg">Coming next</div>
        <div className="mt-1.5 text-sm text-fg-muted">
          The Projects screen lands in the next PR.
        </div>
      </Card>
    </div>
  )
}
