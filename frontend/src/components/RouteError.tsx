import { isRouteErrorResponse, useNavigate, useRouteError } from 'react-router-dom'
import { Button } from './ui/Button'
import { Card } from './ui/Card'

/**
 * Global error boundary for the router. Catches loader/render errors so a single
 * failure shows a friendly recovery screen instead of a blank page.
 */
export function RouteError() {
  const error = useRouteError()
  const navigate = useNavigate()

  let detail = 'Something went wrong. Please try again.'
  if (isRouteErrorResponse(error)) {
    detail = `${error.status} ${error.statusText}`
  } else if (error instanceof Error) {
    detail = error.message
  }

  return (
    <div className="flex min-h-screen items-center justify-center bg-bg p-6">
      <Card className="w-full max-w-md p-10 text-center">
        <div className="text-3xl">⚠️</div>
        <h1 className="mt-3 text-lg font-bold text-fg">Unexpected error</h1>
        <p className="mt-2 text-sm text-fg-muted">{detail}</p>
        <div className="mt-6 flex justify-center gap-2">
          <Button onClick={() => navigate('/')}>Back to start</Button>
          <Button variant="secondary" onClick={() => navigate(0)}>
            Reload
          </Button>
        </div>
      </Card>
    </div>
  )
}
