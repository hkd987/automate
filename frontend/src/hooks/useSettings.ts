import { useState, useCallback } from 'react'

export interface AppSettings {
  defaultSshPort: number
  defaultUser: string
  defaultDaemonPort: number
  autoStartDaemon: boolean
  notifyOnRunComplete: boolean
  theme: 'dark'
}

const STORAGE_KEY = 'automate-settings'

const DEFAULT_SETTINGS: AppSettings = {
  defaultSshPort: 22,
  defaultUser: 'root',
  defaultDaemonPort: 4111,
  autoStartDaemon: true,
  notifyOnRunComplete: true,
  theme: 'dark',
}

function loadSettings(): AppSettings {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    if (raw) {
      return { ...DEFAULT_SETTINGS, ...JSON.parse(raw) }
    }
  } catch {
    // ignore corrupt data
  }
  return { ...DEFAULT_SETTINGS }
}

function saveSettings(settings: AppSettings) {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(settings))
}

export function useSettings(): [AppSettings, <K extends keyof AppSettings>(key: K, value: AppSettings[K]) => void] {
  const [settings, setSettings] = useState<AppSettings>(loadSettings)

  const updateSetting = useCallback(<K extends keyof AppSettings>(key: K, value: AppSettings[K]) => {
    setSettings((prev) => {
      const next = { ...prev, [key]: value }
      saveSettings(next)
      return next
    })
  }, [])

  return [settings, updateSetting]
}
