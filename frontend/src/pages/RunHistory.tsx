import React, { useEffect, useState, useCallback } from 'react'
import { Badge, Button, Skeleton } from '../components/common'
import { VmSelector } from '../components/VmSelector'
import { LogViewer } from '../components/LogViewer'
import { useLogStream } from '../components/useLogStream'
import { useTauriCommand } from '../hooks/useTauriCommand'
import { useSelectedVm } from '../hooks/useSelectedVm'
import type { RunRecord } from '../types'
import { statusVariant } from '../utils/status'

function formatDuration(startedAt: string, finishedAt: string | null): string {
  if (!finishedAt) return '--'
  const ms = new Date(finishedAt).getTime() - new Date(startedAt).getTime()
  if (ms < 1000) return `${ms}ms`
  const secs = Math.floor(ms / 1000)
  if (secs < 60) return `${secs}s`
  const mins = Math.floor(secs / 60)
  return `${mins}m ${secs % 60}s`
}

function RunningLogPanel({ runId }: { runId: string }) {
  const { lines, connected, closed } = useLogStream({ runId, enabled: true })
  return (
    <div>
      <div className="flex items-center gap-2 mb-2">
        <p className="text-xs text-gray-400 font-medium">Live Logs</p>
        {connected && !closed && (
          <span className="inline-block w-2 h-2 bg-green-500 rounded-full" title="Connected" />
        )}
        {closed && (
          <span className="text-xs text-gray-500">(stream ended)</span>
        )}
      </div>
      <LogViewer lines={lines} loading={connected && !closed} />
    </div>
  )
}

export function RunHistory() {
  const { vms, selectedVmId, setSelectedVmId, loading: vmLoading } = useSelectedVm()
  const { data: runs, execute: loadRuns, loading } = useTauriCommand<RunRecord[]>('fetch_run_history')
  const [expandedId, setExpandedId] = useState<string | null>(null)
  const [autoRefresh, setAutoRefresh] = useState(false)

  const refresh = useCallback(() => {
    if (!selectedVmId) return
    loadRuns({ vm_id: selectedVmId }).catch(() => {})
  }, [loadRuns, selectedVmId])

  useEffect(() => {
    refresh()
  }, [refresh])

  useEffect(() => {
    if (!autoRefresh) return
    const interval = setInterval(refresh, 5000)
    return () => clearInterval(interval)
  }, [autoRefresh, refresh])

  const runList = runs ?? []
  const isInitialLoad = loading && runs === null

  return (
    <div>
      <div className="flex items-center justify-between mb-6">
        <div>
          <h1 className="text-2xl font-bold">Run History</h1>
          <p className="text-gray-400 text-sm mt-1">View past automation runs and their results.</p>
        </div>
        <div className="flex items-center gap-4">
          <VmSelector vms={vms} selectedVmId={selectedVmId} onSelect={setSelectedVmId} loading={vmLoading} />
          <label className="text-sm text-gray-300 flex items-center gap-2 cursor-pointer">
            <input
              type="checkbox"
              checked={autoRefresh}
              onChange={(e) => setAutoRefresh(e.target.checked)}
              className="rounded bg-gray-800 border-gray-600"
            />
            Auto-refresh
          </label>
          <Button variant="secondary" onClick={refresh} disabled={loading}>
            {loading ? 'Loading...' : 'Refresh'}
          </Button>
        </div>
      </div>

      {isInitialLoad ? (
        <div className="overflow-x-auto">
          <table className="w-full text-sm text-left">
            <thead className="text-gray-400 border-b border-gray-700">
              <tr>
                <th className="pb-3 font-medium w-8"></th>
                <th className="pb-3 font-medium">Timestamp</th>
                <th className="pb-3 font-medium">Automation</th>
                <th className="pb-3 font-medium">Trigger</th>
                <th className="pb-3 font-medium">Status</th>
                <th className="pb-3 font-medium">Duration</th>
              </tr>
            </thead>
            <tbody>
              <Skeleton variant="table-row" count={5} />
            </tbody>
          </table>
        </div>
      ) : runList.length === 0 ? (
        <div className="text-center py-16 text-gray-500">
          <p className="text-lg mb-2">No runs yet</p>
          <p className="text-sm">Trigger an automation to see results here.</p>
        </div>
      ) : (
        <div className="overflow-x-auto">
          <table className="w-full text-sm text-left">
            <thead className="text-gray-400 border-b border-gray-700">
              <tr>
                <th className="pb-3 font-medium w-8"></th>
                <th className="pb-3 font-medium">Timestamp</th>
                <th className="pb-3 font-medium">Automation</th>
                <th className="pb-3 font-medium">Trigger</th>
                <th className="pb-3 font-medium">Status</th>
                <th className="pb-3 font-medium">Duration</th>
              </tr>
            </thead>
            <tbody className="text-gray-200">
              {runList.map((run) => (
                <React.Fragment key={run.id}>
                  <tr
                    className="border-b border-gray-800 hover:bg-gray-800/50 cursor-pointer"
                    onClick={() => setExpandedId(expandedId === run.id ? null : run.id)}
                  >
                    <td className="py-3 text-gray-500">
                      {expandedId === run.id ? '\u25BC' : '\u25B6'}
                    </td>
                    <td className="py-3 text-gray-400">
                      {new Date(run.started_at).toLocaleString()}
                    </td>
                    <td className="py-3 font-medium">{run.automation_name}</td>
                    <td className="py-3 text-gray-400">{run.trigger_source}</td>
                    <td className="py-3">
                      <Badge variant={statusVariant(run.status)}>{run.status}</Badge>
                    </td>
                    <td className="py-3 text-gray-400">
                      {formatDuration(run.started_at, run.finished_at)}
                    </td>
                  </tr>
                  {expandedId === run.id && (
                    <tr key={`${run.id}-detail`} className="border-b border-gray-800">
                      <td colSpan={6} className="px-4 py-4">
                        {run.status === 'running' ? (
                          <RunningLogPanel runId={run.id} />
                        ) : (
                          <div className="bg-gray-900 rounded-lg border border-gray-700 p-4 space-y-3">
                            {run.output && (
                              <div>
                                <p className="text-xs text-gray-400 mb-1 font-medium">Output</p>
                                <pre className="text-sm text-gray-300 font-mono whitespace-pre-wrap break-words max-h-64 overflow-y-auto">
                                  {run.output}
                                </pre>
                              </div>
                            )}
                            {run.error && (
                              <div>
                                <p className="text-xs text-red-400 mb-1 font-medium">Error</p>
                                <pre className="text-sm text-red-300 font-mono whitespace-pre-wrap break-words">
                                  {run.error}
                                </pre>
                              </div>
                            )}
                            {!run.output && !run.error && (
                              <p className="text-sm text-gray-500">No output available.</p>
                            )}
                          </div>
                        )}
                      </td>
                    </tr>
                  )}
                </React.Fragment>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  )
}
