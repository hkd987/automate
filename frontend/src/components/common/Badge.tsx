type BadgeVariant = 'success' | 'warning' | 'error' | 'info' | 'neutral'

interface BadgeProps {
  variant?: BadgeVariant
  children: React.ReactNode
}

const variantStyles: Record<BadgeVariant, string> = {
  success: 'bg-green-900/50 text-green-400 border-green-700',
  warning: 'bg-yellow-900/50 text-yellow-400 border-yellow-700',
  error: 'bg-red-900/50 text-red-400 border-red-700',
  info: 'bg-blue-900/50 text-blue-400 border-blue-700',
  neutral: 'bg-gray-800 text-gray-400 border-gray-600',
}

export function Badge({ variant = 'neutral', children }: BadgeProps) {
  return (
    <span
      className={`inline-flex items-center px-2 py-0.5 rounded-full text-xs font-medium border ${variantStyles[variant]}`}
    >
      {children}
    </span>
  )
}
