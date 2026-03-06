import { useEffect, useState } from 'react'
import { Input } from '../components/common'
import { useSettings } from '../hooks/useSettings'
import { invoke } from '@tauri-apps/api/core'

interface AppInfo {
  version: string
  platform: string
  build_date: string
}

function Toggle({ checked, onChange, label }: { checked: boolean; onChange: (v: boolean) => void; label: string }) {
  return (
    <label className="flex items-center justify-between cursor-pointer">
      <span className="text-sm text-gray-300">{label}</span>
      <button
        type="button"
        role="switch"
        aria-checked={checked}
        onClick={() => onChange(!checked)}
        className={`relative inline-flex h-6 w-11 items-center rounded-full transition-colors ${
          checked ? 'bg-blue-600' : 'bg-gray-600'
        }`}
      >
        <span
          className={`inline-block h-4 w-4 rounded-full bg-white transition-transform ${
            checked ? 'translate-x-6' : 'translate-x-1'
          }`}
        />
      </button>
    </label>
  )
}

export function Settings() {
  const [settings, updateSetting] = useSettings()
  const [appInfo, setAppInfo] = useState<AppInfo | null>(null)

  useEffect(() => {
    invoke<AppInfo>('get_app_info').then(setAppInfo).catch(() => {})
  }, [])

  return (
    <div>
      <h1 className="text-2xl font-bold mb-6">Settings</h1>

      <div className="space-y-8 max-w-lg">
        <section>
          <h2 className="text-lg font-semibold mb-4 text-gray-300">General</h2>
          <div className="space-y-4">
            <Input
              label="Default SSH Port"
              type="number"
              value={settings.defaultSshPort}
              onChange={(e) => updateSetting('defaultSshPort', Number(e.target.value) || 22)}
            />
            <Input
              label="Default User"
              type="text"
              value={settings.defaultUser}
              onChange={(e) => updateSetting('defaultUser', e.target.value)}
            />
          </div>
        </section>

        <section>
          <h2 className="text-lg font-semibold mb-4 text-gray-300">Daemon</h2>
          <div className="space-y-4">
            <Input
              label="Default Daemon Port"
              type="number"
              value={settings.defaultDaemonPort}
              onChange={(e) => updateSetting('defaultDaemonPort', Number(e.target.value) || 4111)}
            />
            <Toggle
              label="Auto-start daemon on connect"
              checked={settings.autoStartDaemon}
              onChange={(v) => updateSetting('autoStartDaemon', v)}
            />
          </div>
        </section>

        <section>
          <h2 className="text-lg font-semibold mb-4 text-gray-300">Notifications</h2>
          <div className="space-y-4">
            <Toggle
              label="Notify on run completion"
              checked={settings.notifyOnRunComplete}
              onChange={(v) => updateSetting('notifyOnRunComplete', v)}
            />
          </div>
        </section>

        <section>
          <h2 className="text-lg font-semibold mb-4 text-gray-300">About</h2>
          <div className="space-y-2 text-sm text-gray-400">
            <p>Version: {appInfo?.version ?? '0.1.0'}</p>
            <p>Platform: {appInfo?.platform ?? 'unknown'}</p>
            <p>Build: {appInfo?.build_date ?? 'dev'}</p>
            <div className="pt-2 space-x-4">
              <a
                href="https://github.com/anthropics/automate"
                target="_blank"
                rel="noopener noreferrer"
                className="text-blue-400 hover:underline"
              >
                GitHub
              </a>
              <a
                href="https://github.com/anthropics/automate#readme"
                target="_blank"
                rel="noopener noreferrer"
                className="text-blue-400 hover:underline"
              >
                Documentation
              </a>
            </div>
          </div>
        </section>
      </div>
    </div>
  )
}
