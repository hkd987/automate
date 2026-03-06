import { useState, useEffect, useCallback } from 'react'
import { invoke } from '@tauri-apps/api/core'
import type { VmProfile } from '../types'

const STORAGE_KEY = 'automate_selected_vm'

export function useSelectedVm() {
  const [vms, setVms] = useState<VmProfile[]>([])
  const [selectedVmId, setSelectedVmIdState] = useState<string | null>(
    () => localStorage.getItem(STORAGE_KEY)
  )
  const [loading, setLoading] = useState(true)

  const setSelectedVmId = useCallback((id: string | null) => {
    setSelectedVmIdState(id)
    if (id) {
      localStorage.setItem(STORAGE_KEY, id)
    } else {
      localStorage.removeItem(STORAGE_KEY)
    }
  }, [])

  useEffect(() => {
    let cancelled = false
    invoke<VmProfile[]>('list_vms')
      .then((result) => {
        if (cancelled) return
        setVms(result)
        // Auto-select first VM if none selected or selected VM no longer exists
        if (result.length > 0) {
          const stored = localStorage.getItem(STORAGE_KEY)
          if (!stored || !result.some((vm) => vm.id === stored)) {
            setSelectedVmId(result[0].id)
          }
        }
      })
      .catch(() => {})
      .finally(() => {
        if (!cancelled) setLoading(false)
      })
    return () => { cancelled = true }
  }, [setSelectedVmId])

  return { vms, selectedVmId, setSelectedVmId, loading }
}
