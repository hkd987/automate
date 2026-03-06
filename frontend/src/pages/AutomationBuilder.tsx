import { useState, useEffect } from 'react'
import { useSearchParams } from 'react-router-dom'
import { Button, Input } from '../components/common'
import { useTauriCommand } from '../hooks/useTauriCommand'
import type { AutomationDef, AuthProfileDef, VmProfile } from '../types'

type TriggerType = 'manual' | 'cron' | 'webhook' | 'log_pattern'

interface FormState {
  name: string
  triggerType: TriggerType
  cronExpression: string
  logPattern: string
  authProfile: string
  prompt: string
}

interface FormErrors {
  name?: string
  prompt?: string
  cronExpression?: string
  vms?: string
}

const emptyForm: FormState = {
  name: '',
  triggerType: 'manual',
  cronExpression: '0 9 * * *',
  logPattern: 'ERROR',
  authProfile: '',
  prompt: '',
}

const CRON_REGEX = /^(\S+\s+){4}\S+$/

function validateForm(form: FormState): FormErrors {
  const errors: FormErrors = {}
  if (!form.name.trim()) errors.name = 'Name is required'
  if (!form.prompt.trim()) errors.prompt = 'Prompt is required'
  if (form.triggerType === 'cron' && !CRON_REGEX.test(form.cronExpression.trim())) {
    errors.cronExpression = 'Invalid cron expression (expected 5 fields)'
  }
  return errors
}

function formToTriggerString(form: FormState): string {
  switch (form.triggerType) {
    case 'cron': return `cron(${form.cronExpression})`
    case 'webhook': return 'webhook'
    case 'log_pattern': return `log_pattern("${form.logPattern}")`
    case 'manual': return 'manual'
  }
}

function formToYaml(form: FormState): string {
  const lines = [
    'automations:',
    `  - name: ${form.name || 'my-automation'}`,
    `    trigger: ${formToTriggerString(form)}`,
  ]
  if (form.authProfile) {
    lines.push(`    auth_profile: ${form.authProfile}`)
  }
  lines.push(`    prompt: "${form.prompt.replace(/"/g, '\\"')}"`)
  return lines.join('\n')
}

function parseTriggerParam(triggerStr: string): { type: TriggerType; cron?: string; pattern?: string } {
  try {
    const parsed = JSON.parse(triggerStr)
    if (typeof parsed === 'string') {
      if (parsed.startsWith('cron(')) {
        const inner = parsed.slice(5, -1)
        return { type: 'cron', cron: inner }
      }
      if (parsed.startsWith('log_pattern(')) {
        const inner = parsed.slice(12, -1).replace(/^["']|["']$/g, '')
        return { type: 'log_pattern', pattern: inner }
      }
      if (parsed === 'webhook') return { type: 'webhook' }
      return { type: 'manual' }
    }
    return { type: 'manual' }
  } catch {
    return { type: 'manual' }
  }
}

export function AutomationBuilder() {
  const [searchParams] = useSearchParams()
  const { execute: deployAutomation, loading: deploying, error: deployError } =
    useTauriCommand<AutomationDef>('deploy_automation')
  const { data: profiles, execute: loadProfiles } =
    useTauriCommand<Record<string, AuthProfileDef>>('list_auth_profiles')
  const { data: vms, execute: loadVms } =
    useTauriCommand<VmProfile[]>('list_vms')

  const [form, setForm] = useState<FormState>(() => {
    const templateName = searchParams.get('template_name')
    const templateTrigger = searchParams.get('template_trigger')
    const templatePrompt = searchParams.get('template_prompt')
    if (templateName && templatePrompt) {
      const trigger = templateTrigger ? parseTriggerParam(templateTrigger) : { type: 'manual' as TriggerType }
      return {
        ...emptyForm,
        name: templateName,
        triggerType: trigger.type,
        cronExpression: trigger.cron || emptyForm.cronExpression,
        logPattern: trigger.pattern || emptyForm.logPattern,
        prompt: templatePrompt,
      }
    }
    return emptyForm
  })
  const [formErrors, setFormErrors] = useState<FormErrors>({})
  const [showYaml, setShowYaml] = useState(false)
  const [success, setSuccess] = useState(false)
  const [selectedVmIds, setSelectedVmIds] = useState<string[]>([])

  useEffect(() => {
    loadProfiles().catch(() => {})
    loadVms().catch(() => {})
  }, [loadProfiles, loadVms])

  const setField = <K extends keyof FormState>(key: K, value: FormState[K]) => {
    setForm((f) => ({ ...f, [key]: value }))
    setFormErrors((e) => ({ ...e, [key]: undefined }))
    setSuccess(false)
  }

  const handleSave = async () => {
    const errors = validateForm(form)
    setFormErrors(errors)
    if (Object.keys(errors).length > 0) return

    const automation: AutomationDef = {
      name: form.name,
      trigger: formToTriggerString(form),
      auth_profile: form.authProfile || null,
      prompt: form.prompt,
      file: null,
    }
    try {
      if (selectedVmIds.length === 0) {
        setFormErrors((e) => ({ ...e, vms: 'Select at least one target VM' }))
        return
      }
      for (const vmId of selectedVmIds) {
        await deployAutomation({ automation, vm_id: vmId })
      }
      setSuccess(true)
    } catch {
      // error in hook
    }
  }

  const profileNames = profiles ? Object.keys(profiles) : []

  return (
    <div className="max-w-2xl">
      <h1 className="text-2xl font-bold mb-6">Automation Builder</h1>

      <div className="space-y-5">
        <div>
          <Input
            label="Name"
            placeholder="daily-report"
            value={form.name}
            onChange={(e) => setField('name', e.target.value)}
          />
          {formErrors.name && <p className="text-red-400 text-xs mt-1">{formErrors.name}</p>}
        </div>

        <div className="flex flex-col gap-1">
          <label className="text-sm text-gray-300">Trigger Type</label>
          <select
            value={form.triggerType}
            onChange={(e) => setField('triggerType', e.target.value as TriggerType)}
            className="bg-gray-800 border border-gray-600 rounded-lg px-3 py-2 text-sm text-white"
          >
            <option value="manual">Manual</option>
            <option value="cron">Cron</option>
            <option value="webhook">Webhook</option>
            <option value="log_pattern">Log Pattern</option>
          </select>
        </div>

        {form.triggerType === 'cron' && (
          <div>
            <Input
              label="Cron Expression"
              placeholder="0 9 * * *"
              value={form.cronExpression}
              onChange={(e) => setField('cronExpression', e.target.value)}
            />
            {formErrors.cronExpression && <p className="text-red-400 text-xs mt-1">{formErrors.cronExpression}</p>}
          </div>
        )}

        {form.triggerType === 'log_pattern' && (
          <Input
            label="Log Pattern"
            placeholder="ERROR"
            value={form.logPattern}
            onChange={(e) => setField('logPattern', e.target.value)}
          />
        )}

        <div className="flex flex-col gap-1">
          <label className="text-sm text-gray-300">Auth Profile</label>
          <select
            value={form.authProfile}
            onChange={(e) => setField('authProfile', e.target.value)}
            className="bg-gray-800 border border-gray-600 rounded-lg px-3 py-2 text-sm text-white"
          >
            <option value="">None</option>
            {profileNames.map((name) => (
              <option key={name} value={name}>{name}</option>
            ))}
          </select>
        </div>

        <div className="flex flex-col gap-1">
          <label className="text-sm text-gray-300">Prompt</label>
          <textarea
            value={form.prompt}
            onChange={(e) => setField('prompt', e.target.value)}
            placeholder="Describe what the agent should do..."
            rows={6}
            className="bg-gray-800 border border-gray-600 rounded-lg px-3 py-2 text-sm text-white font-mono placeholder-gray-500 focus:outline-none focus:ring-2 focus:ring-blue-500 resize-y"
          />
          {formErrors.prompt && <p className="text-red-400 text-xs mt-1">{formErrors.prompt}</p>}
        </div>

        {vms && vms.length > 0 && (
          <div className="flex flex-col gap-2">
            <label className="text-sm text-gray-300">Target VMs</label>
            <div className="space-y-1">
              {vms.map((vm) => (
                <label key={vm.id} className="flex items-center gap-2 text-sm text-gray-300 cursor-pointer">
                  <input
                    type="checkbox"
                    checked={selectedVmIds.includes(vm.id)}
                    onChange={(e) => {
                      setSelectedVmIds((ids) =>
                        e.target.checked
                          ? [...ids, vm.id]
                          : ids.filter((id) => id !== vm.id)
                      )
                      setFormErrors((err) => ({ ...err, vms: undefined }))
                    }}
                    className="rounded bg-gray-800 border-gray-600"
                  />
                  {vm.name} ({vm.host})
                </label>
              ))}
            </div>
            {formErrors.vms && (
              <p className="text-red-400 text-xs mt-1">{formErrors.vms}</p>
            )}
          </div>
        )}

        <div className="flex items-center gap-3">
          <label className="text-sm text-gray-300 flex items-center gap-2 cursor-pointer">
            <input
              type="checkbox"
              checked={showYaml}
              onChange={(e) => setShowYaml(e.target.checked)}
              className="rounded bg-gray-800 border-gray-600"
            />
            Show YAML
          </label>
        </div>

        {showYaml && (
          <pre className="bg-gray-900 border border-gray-700 rounded-lg p-4 text-sm text-gray-300 font-mono overflow-x-auto whitespace-pre">
            {formToYaml(form)}
          </pre>
        )}

        {deployError && (
          <div className="bg-red-900/30 border border-red-700 rounded p-3 text-red-300 text-sm">
            {deployError}
          </div>
        )}

        {success && (
          <div className="bg-green-900/30 border border-green-700 rounded p-3 text-green-300 text-sm">
            Automation saved successfully.
          </div>
        )}

        <div className="flex gap-3">
          <Button onClick={handleSave} disabled={deploying}>
            {deploying ? 'Saving...' : 'Save & Deploy'}
          </Button>
        </div>
      </div>
    </div>
  )
}
