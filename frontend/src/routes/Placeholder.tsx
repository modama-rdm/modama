import {Card} from '../components/ui/Card'

/** Stand-in for screens still to be built in upcoming PRs. */
export function Placeholder({title}: { title: string }) {
    return (
        <div className="mx-auto max-w-270 px-6 py-8 animate-[fadeIn_0.2s_ease-out]">
            <Card className="p-12 text-center">
                <div className="text-base font-semibold text-fg">{title}</div>
                <div className="mt-1.5 text-sm text-fg-muted">
                    This screen lands in an upcoming PR.
                </div>
            </Card>
        </div>
    )
}
