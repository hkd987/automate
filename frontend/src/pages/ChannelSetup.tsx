import { useState } from 'react'
import { Button, Input } from '../components/common'
import { useTauriCommand } from '../hooks/useTauriCommand'

type Tab = 'slack' | 'whatsapp'

export function ChannelSetup() {
  const [activeTab, setActiveTab] = useState<Tab>('slack')

  return (
    <div>
      <h1 className="text-2xl font-bold mb-6">Channels</h1>

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

      {activeTab === 'slack' ? <SlackConfig /> : <WhatsAppConfig />}
    </div>
  )
}

function SlackConfig() {
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

      <div className="flex items-center gap-3">
        <label className="text-sm text-gray-300">Enabled</label>
        <button
          onClick={() => setEnabled(!enabled)}
          className={`relative inline-flex h-6 w-11 items-center rounded-full transition-colors ${
            enabled ? 'bg-blue-600' : 'bg-gray-600'
          }`}
        >
          <span
            className={`inline-block h-4 w-4 transform rounded-full bg-white transition-transform ${
              enabled ? 'translate-x-6' : 'translate-x-1'
            }`}
          />
        </button>
      </div>

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

function WhatsAppConfig() {
  const [allowedNumbers, setAllowedNumbers] = useState('')
  const [enabled, setEnabled] = useState(false)

  const { execute: configureWhatsapp, loading, error } = useTauriCommand<void>('configure_whatsapp')
  const [success, setSuccess] = useState(false)

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
      })
      setSuccess(true)
    } catch {
      // error is in hook
    }
  }

  return (
    <div className="space-y-4 max-w-lg">
      <p className="text-gray-400 text-sm mb-4">
        Configure WhatsApp integration. This feature is currently in development.
      </p>

      <Input
        label="Allowed Phone Numbers (comma-separated)"
        placeholder="+1234567890, +0987654321"
        value={allowedNumbers}
        onChange={e => setAllowedNumbers(e.target.value)}
      />

      <div className="flex items-center gap-3">
        <label className="text-sm text-gray-300">Enabled</label>
        <button
          onClick={() => setEnabled(!enabled)}
          className={`relative inline-flex h-6 w-11 items-center rounded-full transition-colors ${
            enabled ? 'bg-blue-600' : 'bg-gray-600'
          }`}
        >
          <span
            className={`inline-block h-4 w-4 transform rounded-full bg-white transition-transform ${
              enabled ? 'translate-x-6' : 'translate-x-1'
            }`}
          />
        </button>
      </div>

      <div className="bg-gray-800 rounded-lg p-4 border border-gray-700">
        <p className="text-sm text-gray-400">
          QR Code pairing will appear here once the WhatsApp integration is fully implemented.
        </p>
        <div className="mt-3 w-48 h-48 bg-gray-900 rounded-lg flex items-center justify-center text-gray-600 text-sm border border-dashed border-gray-600">
          QR Placeholder
        </div>
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
