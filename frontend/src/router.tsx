import { Routes, Route } from 'react-router-dom'
import { Dashboard } from './pages/Dashboard'
import { VmManager } from './pages/VmManager'
import { AutomationBuilder } from './pages/AutomationBuilder'
import { DeployView } from './pages/DeployView'
import { ChannelSetup } from './pages/ChannelSetup'
import { RunHistory } from './pages/RunHistory'
import { TemplateLibrary } from './pages/TemplateLibrary'
import { Settings } from './pages/Settings'

export function AppRouter() {
  return (
    <Routes>
      <Route path="/" element={<Dashboard />} />
      <Route path="/vms" element={<VmManager />} />
      <Route path="/automations" element={<AutomationBuilder />} />
      <Route path="/templates" element={<TemplateLibrary />} />
      <Route path="/deploy" element={<DeployView />} />
      <Route path="/channels" element={<ChannelSetup />} />
      <Route path="/history" element={<RunHistory />} />
      <Route path="/settings" element={<Settings />} />
    </Routes>
  )
}
