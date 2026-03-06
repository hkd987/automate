export interface VmProfile {
  id: string
  name: string
  host: string
  port: number
  user: string
  key_path: string
  arch: string | null
  created_at: string
  updated_at: string
}

export type RunStatus = 'pending' | 'running' | 'completed' | 'failed'

export interface RunRecord {
  id: string
  automation_name: string
  status: RunStatus
  trigger_source: string
  started_at: string
  finished_at: string | null
  output: string | null
  error: string | null
}

export type TriggerDef =
  | { type: 'cron'; expression: string }
  | { type: 'webhook' }
  | { type: 'log_pattern'; pattern: string }
  | { type: 'manual' }

export interface AutomationDef {
  name: string
  trigger: TriggerDef
  auth_profile: string | null
  prompt: string
  file: string | null
}

export type Runtime = 'claude_code' | 'codex'
export type AuthMode = 'subscription' | 'bedrock' | 'api_key'

export interface AuthProfileDef {
  runtime: Runtime
  mode: AuthMode
  aws_region: string | null
  aws_model: string | null
}

export interface ChannelConfig {
  channel_type: 'slack' | 'whatsapp'
  enabled: boolean
  allowlist: string[]
}

export type TemplateCategory = 'monitoring' | 'dev_ops' | 'code_review' | 'reporting' | 'custom'

export interface Template {
  id: string
  name: string
  description: string
  category: TemplateCategory
  config: AutomationDef
}
