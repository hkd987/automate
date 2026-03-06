import { useState, useEffect } from 'react'
import { useTauriCommand } from '../hooks/useTauriCommand'
import type { VmProfile } from '../types'

type StepStatus = 'pending' | 'in_progress' | 'done' | 'error'

interface DeployStep {
  step: number
  label: string
  status: StepStatus
  detail: string | null
}

function StatusIcon({ status }: { status: StepStatus }) {
  switch (status) {
    case 'pending':
      return <span className="text-gray-500">&#9675;</span>
    case 'in_progress':
      return <span className="text-blue-400 animate-pulse">&#9679;</span>
    case 'done':
      return <span className="text-green-400">&#10003;</span>
    case 'error':
      return <span className="text-red-400">&#10007;</span>
  }
}

export function DeployView() {
  const { data: vms, execute: loadVms, loading: loadingVms } = useTauriCommand<VmProfile[]>('list_vms')
  const { execute: deploy, loading: deploying, error: deployError } = useTauriCommand<DeployStep[]>('deploy_daemon')
  const [selectedVm, setSelectedVm] = useState<string>('')
  const [steps, setSteps] = useState<DeployStep[]>([])

  useEffect(() => {
    loadVms()
  }, [loadVms])

  const handleDeploy = async () => {
    if (!selectedVm) return
    setSteps([])
    try {
      const result = await deploy({ vm_id: selectedVm })
      if (result) setSteps(result)
    } catch {
      // error is captured in the hook
    }
  }

  return (
    <div>
      <h1 className="text-2xl font-bold mb-4">Deploy Daemon</h1>
      <p className="text-gray-400 mb-6">Deploy the automate daemon to a remote VM.</p>

      <div className="flex gap-4 items-end mb-8">
        <div className="flex-1">
          <label className="block text-sm font-medium text-gray-300 mb-1">Select VM</label>
          <select
            value={selectedVm}
            onChange={(e) => setSelectedVm(e.target.value)}
            disabled={loadingVms || deploying}
            className="w-full bg-gray-800 border border-gray-700 rounded px-3 py-2 text-white"
          >
            <option value="">-- Select a VM --</option>
            {vms?.map((vm) => (
              <option key={vm.id} value={vm.id}>
                {vm.name} ({vm.host}:{vm.port})
              </option>
            ))}
          </select>
        </div>
        <button
          onClick={handleDeploy}
          disabled={!selectedVm || deploying}
          className="px-4 py-2 bg-blue-600 hover:bg-blue-700 disabled:bg-gray-700 disabled:text-gray-500 rounded text-white font-medium"
        >
          {deploying ? 'Deploying...' : 'Deploy'}
        </button>
      </div>

      {deployError && (
        <div className="bg-red-900/30 border border-red-700 rounded p-3 mb-4 text-red-300">
          {deployError}
        </div>
      )}

      {steps.length > 0 && (
        <div className="bg-gray-800 rounded border border-gray-700 divide-y divide-gray-700">
          {steps.map((s) => (
            <div key={s.step} className="flex items-center gap-3 px-4 py-3">
              <StatusIcon status={s.status} />
              <span className="font-medium text-white">{s.label}</span>
              {s.detail && (
                <span className="text-sm text-gray-400 ml-auto">{s.detail}</span>
              )}
            </div>
          ))}
        </div>
      )}
    </div>
  )
}
