import { useState, useEffect, useRef } from 'react'
import { QRCodeSVG } from 'qrcode.react'
import { Button, Input, Toggle } from '../components/common'
import { VmSelector } from '../components/VmSelector'
import { useTauriCommand } from '../hooks/useTauriCommand'
import { useSelectedVm } from '../hooks/useSelectedVm'

type Tab = 'slack' | 'whatsapp'

export function ChannelSetup() {
  const [activeTab, setActiveTab] = useState<Tab>('slack')
  const { vms, selectedVmId, setSelectedVmId, loading: vmLoading } = useSelectedVm()

  return (
    <div>
      <div className="flex items-center justify-between mb-6">
        <h1 className="text-2xl font-bold">Channels</h1>
        <VmSelector vms={vms} selectedVmId={selectedVmId} onSelect={setSelectedVmId} loading={vmLoading} />
      </div>

      <div className="flex gap-2 mb-6 border-b border-gray-700 pb-2">
        <button
          className={`px-4 py-2 text-sm rounded-t-lg transition-colors ${
            activeTab === 'slack'
              ? 'bg-gray-800 text-white border-b-2 border-blue-500'
              : 'text-gray-400 hover:text-gray-200'
          }`}
          onClick={() => setActiveTab('slack')}
        >
          Slack
        </button>
        <button
          className={`px-4 py-2 text-sm rounded-t-lg transition-colors ${
            activeTab === 'whatsapp'
              ? 'bg-gray-800 text-white border-b-2 border-blue-500'
              : 'text-gray-400 hover:text-gray-200'
          }`}
          onClick={() => setActiveTab('whatsapp')}
        >
          WhatsApp
        </button>
      </div>

      {activeTab === 'slack' ? <SlackConfig vmId={selectedVmId} /> : <WhatsAppConfig vmId={selectedVmId} />}
    </div>
  )
}

function SlackConfig({ vmId }: { vmId: string | null }) {
  const [botToken, setBotToken] = useState('')
  const [appToken, setAppToken] = useState('')
  const [allowedUsers, setAllowedUsers] = useState('')
  const [enabled, setEnabled] = useState(false)

  const { execute: configureSlack, loading, error } = useTauriCommand<void>('configure_slack')
  const [success, setSuccess] = useState(false)

  const handleSave = async () => {
    setSuccess(false)
    try {
      const userIds = allowedUsers
        .split(',')
        .map(s => s.trim())
        .filter(s => s.length > 0)

      await configureSlack({
        botToken,
        appToken,
        allowedUserIds: userIds,
        enabled,
        vm_id: vmId,
      })
      setSuccess(true)
    } catch {
      // error is in hook
    }
  }

  const handleTestConnection = async () => {
    // For now, just validate inputs
    if (!botToken || !appToken) {
      alert('Both Bot Token and App Token are required to test the connection.')
      return
    }
    alert('Connection test is not yet implemented. Tokens look valid.')
  }

  return (
    <div className="space-y-4 max-w-lg">
      <p className="text-gray-400 text-sm mb-4">
        Connect your Slack workspace using Socket Mode. You need a bot token (xoxb-) and an app-level token (xapp-).
      </p>

      <Input
        label="Bot Token"
        placeholder="xoxb-..."
        type="password"
        value={botToken}
        onChange={e => setBotToken(e.target.value)}
      />

      <Input
        label="App Token (Socket Mode)"
        placeholder="xapp-..."
        type="password"
        value={appToken}
        onChange={e => setAppToken(e.target.value)}
      />

      <Input
        label="Allowed User IDs (comma-separated)"
        placeholder="U01ABC123, U02DEF456"
        value={allowedUsers}
        onChange={e => setAllowedUsers(e.target.value)}
      />

      <Toggle label="Enabled" checked={enabled} onChange={setEnabled} />

      {error && <p className="text-red-400 text-sm">{error}</p>}
      {success && <p className="text-green-400 text-sm">Slack configuration saved.</p>}

      <div className="flex gap-3 pt-2">
        <Button onClick={handleSave} disabled={loading}>
          {loading ? 'Saving...' : 'Save Configuration'}
        </Button>
        <Button variant="secondary" onClick={handleTestConnection}>
          Test Connection
        </Button>
      </div>
    </div>
  )
}

type WhatsAppStatus = 'idle' | 'waiting_qr' | 'connected' | 'disconnected'

function WhatsAppConfig({ vmId }: { vmId: string | null }) {
  const [allowedNumbers, setAllowedNumbers] = useState('')
  const [enabled, setEnabled] = useState(false)
  const [qrCode, setQrCode] = useState<string | null>(null)
  const [status, setStatus] = useState<WhatsAppStatus>('idle')
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null)

  const { execute: configureWhatsapp, loading, error } = useTauriCommand<void>('configure_whatsapp')
  const { execute: fetchDaemonApi } = useTauriCommand<string>('daemon_api_get')
  const [success, setSuccess] = useState(false)

  // Derive idle state from enabled/vmId — no effect needed for reset
  const isActive = enabled && !!vmId

  // Poll for QR code when WhatsApp is enabled
  useEffect(() => {
    if (!isActive) {
      return
    }

    let cancelled = false

    const poll = async () => {
      try {
        const resp = await fetchDaemonApi({ path: '/channels/whatsapp/qr', vm_id: vmId! })
        if (!cancelled && resp) {
          const data = JSON.parse(resp)
          setQrCode(data.qr)
          setStatus('waiting_qr')
        }
      } catch {
        if (!cancelled) {
          // 404 means no QR available (already authenticated or not started)
          setQrCode(prev => {
            if (prev) {
              setStatus('connected')
            }
            return null
          })
        }
      }
    }

    poll()
    intervalRef.current = setInterval(poll, 2000)

    return () => {
      cancelled = true
      if (intervalRef.current) {
        clearInterval(intervalRef.current)
        intervalRef.current = null
      }
      setQrCode(null)
      setStatus('idle')
    }
  }, [isActive, vmId, fetchDaemonApi])

  const handleSave = async () => {
    setSuccess(false)
    try {
      const numbers = allowedNumbers
        .split(',')
        .map(s => s.trim())
        .filter(s => s.length > 0)

      await configureWhatsapp({
        allowedNumbers: numbers,
        enabled,
        vm_id: vmId,
      })
      setSuccess(true)
    } catch {
      // error is in hook
    }
  }

  const statusLabel = {
    idle: 'Not started',
    waiting_qr: 'Waiting for QR scan...',
    connected: 'Connected',
    disconnected: 'Disconnected',
  }

  return (
    <div className="space-y-4 max-w-lg">
      <p className="text-gray-400 text-sm mb-4">
        Configure WhatsApp integration. Scan the QR code with your phone to pair.
      </p>

      <Input
        label="Allowed Phone Numbers (comma-separated)"
        placeholder="+1234567890, +0987654321"
        value={allowedNumbers}
        onChange={e => setAllowedNumbers(e.target.value)}
      />

      <Toggle label="Enabled" checked={enabled} onChange={setEnabled} />

      <div className="bg-gray-800 rounded-lg p-4 border border-gray-700">
        <div className="flex items-center gap-2 mb-3">
          <div className={`w-2 h-2 rounded-full ${
            status === 'connected' ? 'bg-green-500' :
            status === 'waiting_qr' ? 'bg-yellow-500 animate-pulse' :
            status === 'disconnected' ? 'bg-red-500' :
            'bg-gray-500'
          }`} />
          <p className="text-sm text-gray-400">{statusLabel[status]}</p>
        </div>

        {qrCode ? (
          <div className="bg-white rounded-lg p-3 inline-block">
            <QRCodeSVG value={qrCode} size={192} />
          </div>
        ) : (
          <div className="w-48 h-48 bg-gray-900 rounded-lg flex items-center justify-center text-gray-600 text-sm border border-dashed border-gray-600">
            {status === 'connected' ? 'Paired' : 'QR code will appear here'}
          </div>
        )}
      </div>

      {error && <p className="text-red-400 text-sm">{error}</p>}
      {success && <p className="text-green-400 text-sm">WhatsApp configuration saved.</p>}

      <div className="flex gap-3 pt-2">
        <Button onClick={handleSave} disabled={loading}>
          {loading ? 'Saving...' : 'Save Configuration'}
        </Button>
      </div>
    </div>
  )
}
