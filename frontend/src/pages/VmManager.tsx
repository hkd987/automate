import { useEffect, useState, useCallback } from 'react'
import { Button, Input, Modal, Badge, Skeleton, ConfirmDialog } from '../components/common'
import { useTauriCommand } from '../hooks/useTauriCommand'
import type { VmProfile } from '../types'

interface VmForm {
  name: string
  host: string
  port: string
  user: string
  key_path: string
}

interface FormErrors {
  name?: string
  host?: string
  port?: string
}

const emptyForm: VmForm = { name: '', host: '', port: '22', user: 'root', key_path: '' }

function validateForm(form: VmForm): FormErrors {
  const errors: FormErrors = {}
  if (!form.name.trim()) errors.name = 'Name is required'
  if (!form.host.trim()) errors.host = 'Host is required'
  const port = parseInt(form.port, 10)
  if (isNaN(port) || port < 1 || port > 65535) errors.port = 'Port must be 1-65535'
  return errors
}

export function VmManager() {
  const { data: vms, execute: fetchVms, loading: loadingVms } = useTauriCommand<VmProfile[]>('list_vms')
  const { execute: addVm } = useTauriCommand<VmProfile>('add_vm')
  const { execute: updateVm } = useTauriCommand<VmProfile>('update_vm')
  const { execute: removeVm } = useTauriCommand<void>('delete_vm')
  const { execute: testConn, loading: testing } = useTauriCommand<string>('test_connection')

  const [modalOpen, setModalOpen] = useState(false)
  const [editingId, setEditingId] = useState<string | null>(null)
  const [form, setForm] = useState<VmForm>(emptyForm)
  const [formErrors, setFormErrors] = useState<FormErrors>({})
  const [testResult, setTestResult] = useState<{ id: string; ok: boolean; msg: string } | null>(null)
  const [deleteTarget, setDeleteTarget] = useState<{ id: string; name: string } | null>(null)

  const loadVms = useCallback(() => {
    fetchVms().catch(() => {})
  }, [fetchVms])

  useEffect(() => {
    loadVms()
  }, [loadVms])

  const openAdd = () => {
    setEditingId(null)
    setForm(emptyForm)
    setFormErrors({})
    setModalOpen(true)
  }

  const openEdit = (vm: VmProfile) => {
    setEditingId(vm.id)
    setForm({
      name: vm.name,
      host: vm.host,
      port: String(vm.port),
      user: vm.user,
      key_path: vm.key_path,
    })
    setFormErrors({})
    setModalOpen(true)
  }

  const handleSubmit = async () => {
    const errors = validateForm(form)
    setFormErrors(errors)
    if (Object.keys(errors).length > 0) return

    const port = parseInt(form.port, 10) || 22
    try {
      if (editingId) {
        await updateVm({ id: editingId, name: form.name, host: form.host, port, user: form.user, key_path: form.key_path })
      } else {
        await addVm({ name: form.name, host: form.host, port, user: form.user, key_path: form.key_path })
      }
      setModalOpen(false)
      loadVms()
    } catch {
      // error is in the hook
    }
  }

  const handleDelete = async (id: string) => {
    try {
      await removeVm({ id })
      loadVms()
    } catch {
      // error is in the hook
    }
  }

  const handleTest = async (vm: VmProfile) => {
    setTestResult(null)
    try {
      const msg = await testConn({ host: vm.host, port: vm.port, user: vm.user, key_path: vm.key_path })
      setTestResult({ id: vm.id, ok: true, msg: msg ?? 'OK' })
    } catch (err) {
      setTestResult({ id: vm.id, ok: false, msg: String(err) })
    }
  }

  const setField = (field: keyof VmForm, value: string) => {
    setForm((f) => ({ ...f, [field]: value }))
    setFormErrors((e) => ({ ...e, [field]: undefined }))
  }

  const vmList = vms ?? []
  const isInitialLoad = loadingVms && vms === null

  return (
    <div>
      <div className="flex items-center justify-between mb-6">
        <div>
          <h1 className="text-2xl font-bold">VM Manager</h1>
          <p className="text-gray-400 text-sm mt-1">Manage your SSH-accessible VMs.</p>
        </div>
        <Button onClick={openAdd}>Add VM</Button>
      </div>

      {isInitialLoad ? (
        <div className="overflow-x-auto">
          <table className="w-full text-sm text-left">
            <thead className="text-gray-400 border-b border-gray-700">
              <tr>
                <th className="pb-3 font-medium">Name</th>
                <th className="pb-3 font-medium">Host</th>
                <th className="pb-3 font-medium">User</th>
                <th className="pb-3 font-medium">Arch</th>
                <th className="pb-3 font-medium">Status</th>
                <th className="pb-3 font-medium text-right">Actions</th>
              </tr>
            </thead>
            <tbody>
              <Skeleton variant="table-row" count={3} />
            </tbody>
          </table>
        </div>
      ) : vmList.length === 0 ? (
        <div className="text-center py-16 text-gray-500">
          <p className="text-lg mb-2">No VMs configured</p>
          <p className="text-sm">Add a VM to get started with deployments.</p>
        </div>
      ) : (
        <div className="overflow-x-auto">
          <table className="w-full text-sm text-left">
            <thead className="text-gray-400 border-b border-gray-700">
              <tr>
                <th className="pb-3 font-medium">Name</th>
                <th className="pb-3 font-medium">Host</th>
                <th className="pb-3 font-medium">User</th>
                <th className="pb-3 font-medium">Arch</th>
                <th className="pb-3 font-medium">Status</th>
                <th className="pb-3 font-medium text-right">Actions</th>
              </tr>
            </thead>
            <tbody className="text-gray-200">
              {vmList.map((vm) => (
                <tr key={vm.id} className="border-b border-gray-800 hover:bg-gray-800/50">
                  <td className="py-3 font-medium">{vm.name}</td>
                  <td className="py-3 text-gray-400">{vm.host}:{vm.port}</td>
                  <td className="py-3 text-gray-400">{vm.user}</td>
                  <td className="py-3">
                    {vm.arch ? (
                      <Badge variant="info">{vm.arch}</Badge>
                    ) : (
                      <Badge variant="neutral">unknown</Badge>
                    )}
                  </td>
                  <td className="py-3">
                    {testResult?.id === vm.id ? (
                      <Badge variant={testResult.ok ? 'success' : 'error'}>
                        {testResult.ok ? 'connected' : 'failed'}
                      </Badge>
                    ) : (
                      <Badge variant="neutral">untested</Badge>
                    )}
                  </td>
                  <td className="py-3 text-right space-x-2">
                    <Button
                      variant="secondary"
                      className="px-3 py-1 text-xs"
                      onClick={() => handleTest(vm)}
                      disabled={testing}
                    >
                      {testing && testResult?.id === vm.id ? 'Testing...' : 'Test'}
                    </Button>
                    <Button
                      variant="secondary"
                      className="px-3 py-1 text-xs"
                      onClick={() => openEdit(vm)}
                    >
                      Edit
                    </Button>
                    <Button
                      variant="danger"
                      className="px-3 py-1 text-xs"
                      onClick={() => setDeleteTarget({ id: vm.id, name: vm.name })}
                    >
                      Delete
                    </Button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

      <ConfirmDialog
        open={deleteTarget !== null}
        onClose={() => setDeleteTarget(null)}
        onConfirm={() => {
          if (deleteTarget) handleDelete(deleteTarget.id)
        }}
        title="Delete VM"
        message={`Are you sure you want to delete "${deleteTarget?.name ?? ''}"? This action cannot be undone.`}
        confirmLabel="Delete"
        danger
      />

      <Modal open={modalOpen} onClose={() => setModalOpen(false)} title={editingId ? 'Edit VM' : 'Add VM'}>
        <div className="flex flex-col gap-4">
          <div>
            <Input label="Name" placeholder="my-server" value={form.name} onChange={(e) => setField('name', e.target.value)} />
            {formErrors.name && <p className="text-red-400 text-xs mt-1">{formErrors.name}</p>}
          </div>
          <div className="grid grid-cols-3 gap-3">
            <div className="col-span-2">
              <Input label="Host" placeholder="192.168.1.100" value={form.host} onChange={(e) => setField('host', e.target.value)} />
              {formErrors.host && <p className="text-red-400 text-xs mt-1">{formErrors.host}</p>}
            </div>
            <div>
              <Input label="Port" type="number" value={form.port} onChange={(e) => setField('port', e.target.value)} />
              {formErrors.port && <p className="text-red-400 text-xs mt-1">{formErrors.port}</p>}
            </div>
          </div>
          <Input label="User" placeholder="root" value={form.user} onChange={(e) => setField('user', e.target.value)} />
          <Input label="SSH Key Path" placeholder="~/.ssh/id_rsa" value={form.key_path} onChange={(e) => setField('key_path', e.target.value)} />
          <div className="flex justify-end gap-3 mt-2">
            <Button variant="secondary" onClick={() => setModalOpen(false)}>Cancel</Button>
            <Button onClick={handleSubmit}>{editingId ? 'Update' : 'Add'}</Button>
          </div>
        </div>
      </Modal>
    </div>
  )
}
