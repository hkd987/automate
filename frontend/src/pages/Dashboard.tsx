import { useEffect } from 'react'
import { Badge, Button, Skeleton } from '../components/common'
import { useTauriCommand } from '../hooks/useTauriCommand'
import type { AutomationDef, RunRecord, RunStatus } from '../types'
import { Link } from 'react-router-dom'

function statusVariant(status: RunStatus) {
  switch (status) {
    case 'completed': return 'success' as const
    case 'failed': return 'error' as const
    case 'running': return 'info' as const
    case 'pending': return 'neutral' as const
  }
}

function triggerLabel(trigger: string): string {
  if (trigger.startsWith('cron(')) return 'Cron'
  if (trigger === 'webhook') return 'Webhook'
  if (trigger.startsWith('log_pattern(')) return 'Log Pattern'
  if (trigger === 'manual') return 'Manual'
  return trigger
}

export function Dashboard() {
  const { data: automations, execute: loadAutomations, loading: loadingAutos } = useTauriCommand<AutomationDef[]>('list_remote_automations')
  const { data: runs, execute: loadRuns, loading: loadingRuns } = useTauriCommand<RunRecord[]>('fetch_run_history')
  const { execute: triggerRun, loading: triggering } = useTauriCommand<string>('trigger_remote_run')

  useEffect(() => {
    loadAutomations().catch(() => {})
    loadRuns().catch(() => {})
  }, [loadAutomations, loadRuns])

  const handleRunNow = async (name: string) => {
    try {
      await triggerRun({ name })
      await loadRuns()
    } catch {
      // error in hook
    }
  }

  const automationList = automations ?? []
  const runList = (runs ?? []).slice(0, 5)
  const isAutosInitial = loadingAutos && automations === null
  const isRunsInitial = loadingRuns && runs === null

  return (
    <div>
      <h1 className="text-2xl font-bold mb-6">Dashboard</h1>

      <section className="mb-8">
        <h2 className="text-lg font-semibold mb-3 text-gray-300">Automations</h2>
        {isAutosInitial ? (
          <div className="overflow-x-auto">
            <table className="w-full text-sm text-left">
              <thead className="text-gray-400 border-b border-gray-700">
                <tr>
                  <th className="pb-3 font-medium">Name</th>
                  <th className="pb-3 font-medium">Trigger</th>
                  <th className="pb-3 font-medium">Auth Profile</th>
                  <th className="pb-3 font-medium text-right">Actions</th>
                </tr>
              </thead>
              <tbody>
                <Skeleton variant="table-row" count={3} />
              </tbody>
            </table>
          </div>
        ) : automationList.length === 0 ? (
          <div className="text-center py-12 text-gray-500">
            <p className="mb-1">No automations configured</p>
            <p className="text-sm">
              Create one in the{' '}
              <Link to="/automations" className="text-blue-400 hover:underline">
                Automations
              </Link>{' '}
              page.
            </p>
          </div>
        ) : (
          <div className="overflow-x-auto">
            <table className="w-full text-sm text-left">
              <thead className="text-gray-400 border-b border-gray-700">
                <tr>
                  <th className="pb-3 font-medium">Name</th>
                  <th className="pb-3 font-medium">Trigger</th>
                  <th className="pb-3 font-medium">Auth Profile</th>
                  <th className="pb-3 font-medium text-right">Actions</th>
                </tr>
              </thead>
              <tbody className="text-gray-200">
                {automationList.map((auto) => (
                  <tr key={auto.name} className="border-b border-gray-800 hover:bg-gray-800/50">
                    <td className="py-3 font-medium">{auto.name}</td>
                    <td className="py-3">
                      <Badge variant="info">{typeof auto.trigger === 'string' ? triggerLabel(auto.trigger) : auto.trigger.type ?? 'unknown'}</Badge>
                    </td>
                    <td className="py-3 text-gray-400">{auto.auth_profile ?? 'none'}</td>
                    <td className="py-3 text-right">
                      <Button
                        variant="secondary"
                        className="px-3 py-1 text-xs"
                        onClick={() => handleRunNow(auto.name)}
                        disabled={triggering}
                      >
                        Run Now
                      </Button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </section>

      <section>
        <h2 className="text-lg font-semibold mb-3 text-gray-300">Recent Runs</h2>
        {isRunsInitial ? (
          <div className="overflow-x-auto">
            <table className="w-full text-sm text-left">
              <thead className="text-gray-400 border-b border-gray-700">
                <tr>
                  <th className="pb-3 font-medium">Automation</th>
                  <th className="pb-3 font-medium">Trigger</th>
                  <th className="pb-3 font-medium">Status</th>
                  <th className="pb-3 font-medium">Started</th>
                </tr>
              </thead>
              <tbody>
                <Skeleton variant="table-row" count={3} />
              </tbody>
            </table>
          </div>
        ) : runList.length === 0 ? (
          <p className="text-gray-500 text-sm">No runs yet.</p>
        ) : (
          <div className="overflow-x-auto">
            <table className="w-full text-sm text-left">
              <thead className="text-gray-400 border-b border-gray-700">
                <tr>
                  <th className="pb-3 font-medium">Automation</th>
                  <th className="pb-3 font-medium">Trigger</th>
                  <th className="pb-3 font-medium">Status</th>
                  <th className="pb-3 font-medium">Started</th>
                </tr>
              </thead>
              <tbody className="text-gray-200">
                {runList.map((run) => (
                  <tr key={run.id} className="border-b border-gray-800">
                    <td className="py-3 font-medium">{run.automation_name}</td>
                    <td className="py-3 text-gray-400">{run.trigger_source}</td>
                    <td className="py-3">
                      <Badge variant={statusVariant(run.status)}>{run.status}</Badge>
                    </td>
                    <td className="py-3 text-gray-400">{new Date(run.started_at).toLocaleString()}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </section>
    </div>
  )
}
