import type {ButtonHTMLAttributes} from 'react'

type Variant = 'primary' | 'secondary' | 'danger'

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
    variant?: Variant
    fullWidth?: boolean
}

const base =
    'inline-flex items-center justify-center gap-2 rounded-lg px-5 py-2.5 text-sm font-semibold ' +
    'transition-colors disabled:opacity-60 disabled:cursor-not-allowed focus:outline-none ' +
    'focus:shadow-[0_0_0_3px_var(--ring)]'

const variants: Record<Variant, string> = {
    primary: 'bg-primary text-primary-fg hover:bg-primary-hover',
    secondary:
        'border border-default bg-surface text-fg-muted hover:bg-surface-muted',
    danger: 'bg-danger text-white hover:bg-danger-hover',
}

export function Button({
                           variant = 'primary',
                           fullWidth = false,
                           className = '',
                           type = 'button',
                           ...props
                       }: ButtonProps) {
    return (
        <button
            type={type}
            className={`${base} ${variants[variant]} ${fullWidth ? 'w-full' : ''} ${className}`}
            {...props}
        />
    )
}
