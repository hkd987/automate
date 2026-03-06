import type { RunStatus } from '../types'

export function statusVariant(status: RunStatus) {
  switch (status) {
    case 'completed': return 'success' as const
    case 'failed': return 'error' as const
    case 'running': return 'info' as const
    case 'pending': return 'neutral' as const
  }
}
