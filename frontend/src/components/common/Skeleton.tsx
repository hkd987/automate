type Variant = 'line' | 'card' | 'table-row'

interface SkeletonProps {
  variant?: Variant
  count?: number
}

function SkeletonLine() {
  return <div className="h-4 bg-gray-700 rounded animate-pulse w-full" />
}

function SkeletonCard() {
  return (
    <div className="bg-gray-800 border border-gray-700 rounded-lg p-4 space-y-3 animate-pulse">
      <div className="h-4 bg-gray-700 rounded w-3/4" />
      <div className="h-3 bg-gray-700 rounded w-1/2" />
      <div className="h-3 bg-gray-700 rounded w-5/6" />
    </div>
  )
}

function SkeletonTableRow() {
  return (
    <tr className="border-b border-gray-800">
      <td className="py-3" colSpan={100}>
        <div className="flex gap-4 animate-pulse">
          <div className="h-4 bg-gray-700 rounded w-1/4" />
          <div className="h-4 bg-gray-700 rounded w-1/6" />
          <div className="h-4 bg-gray-700 rounded w-1/5" />
          <div className="h-4 bg-gray-700 rounded w-1/6" />
        </div>
      </td>
    </tr>
  )
}

export function Skeleton({ variant = 'line', count = 1 }: SkeletonProps) {
  const items = Array.from({ length: count }, (_, i) => i)

  if (variant === 'card') {
    return (
      <div className="space-y-4">
        {items.map((i) => (
          <SkeletonCard key={i} />
        ))}
      </div>
    )
  }

  if (variant === 'table-row') {
    return (
      <>
        {items.map((i) => (
          <SkeletonTableRow key={i} />
        ))}
      </>
    )
  }

  return (
    <div className="space-y-3">
      {items.map((i) => (
        <SkeletonLine key={i} />
      ))}
    </div>
  )
}
