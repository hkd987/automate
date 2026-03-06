import { useEffect } from 'react'
import { Badge, Button, Skeleton } from '../components/common'
import { VmSelector } from '../components/VmSelector'
import { useTauriCommand } from '../hooks/useTauriCommand'
import { useSelectedVm } from '../hooks/useSelectedVm'
import type { AutomationDef, RunRecord } from '../types'
import { statusVariant } from '../utils/status'
import { Link } from 'react-router-dom'

function triggerLabel(trigger: string): string {
  if (trigger.startsWith('cron(')) return 'Cron'
  if (trigger === 'webhook') return 'Webhook'
  if (trigger.startsWith('log_pattern(')) return 'Log Pattern'
  if (trigger === 'manual') return 'Manual'
  return trigger
}

export function Dashboard() {
  const { vms, selectedVmId, setSelectedVmId, loading: vmLoading } = useSelectedVm()
  const { data: automations, execute: loadAutomations, loading: loadingAutos } = useTauriCommand<AutomationDef[]>('list_remote_automations')
  const { data: runs, execute: loadRuns, loading: loadingRuns } = useTauriCommand<RunRecord[]>('fetch_run_history')
  const { execute: triggerRun, loading: triggering } = useTauriCommand<string>('trigger_remote_run')

  useEffect(() => {
    if (!selectedVmId) return
    loadAutomations({ vm_id: selectedVmId }).catch(() => {})
    loadRuns({ vm_id: selectedVmId }).catch(() => {})
  }, [loadAutomations, loadRuns, selectedVmId])

  const handleRunNow = async (name: string) => {
    if (!selectedVmId) return
    try {
      await triggerRun({ name, vm_id: selectedVmId })
      await loadRuns({ vm_id: selectedVmId })
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
      <div className="flex items-center justify-between mb-6">
        <h1 className="text-2xl font-bold">Dashboard</h1>
        <VmSelector vms={vms} selectedVmId={selectedVmId} onSelect={setSelectedVmId} loading={vmLoading} />
      </div>

      {!selectedVmId && !vmLoading && vms.length === 0 && (
        <p className="text-gray-500 text-sm mb-6">No VMs configured. <Link to="/vms" className="text-blue-400 hover:underline">Add a VM</Link> to get started.</p>
      )}

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
                      <Badge variant="info">{triggerLabel(auto.trigger)}</Badge>
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
