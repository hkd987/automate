import { Link } from 'react-router-dom'
import type { VmProfile } from '../types'

interface VmSelectorProps {
  vms: VmProfile[]
  selectedVmId: string | null
  onSelect: (id: string) => void
  loading?: boolean
}

export function VmSelector({ vms, selectedVmId, onSelect, loading }: VmSelectorProps) {
  if (loading) {
    return (
      <div className="h-8 w-40 bg-gray-800 rounded animate-pulse" />
    )
  }

  if (vms.length === 0) {
    return (
      <Link to="/vms" className="text-sm text-blue-400 hover:underline">
        Select a VM
      </Link>
    )
  }

  return (
    <select
      value={selectedVmId ?? ''}
      onChange={(e) => onSelect(e.target.value)}
      className="bg-gray-800 border border-gray-600 rounded-lg px-3 py-1.5 text-sm text-white"
    >
      {vms.map((vm) => (
        <option key={vm.id} value={vm.id}>
          {vm.name} ({vm.host})
        </option>
      ))}
    </select>
  )
}
