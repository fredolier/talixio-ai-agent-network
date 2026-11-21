// Ce que renvoie l'API du maître. Tout texte venu de la grappe est affiché comme du texte.

export type Status = 'starting' | 'idle' | 'working' | 'suspended' | 'disconnected' | 'stopped'

export interface ExecRule {
  program: string
  args?: string[] | null
  cwd?: string | null
}

export interface Capabilities {
  fs_read?: string[]
  fs_write?: string[]
  exec?: ExecRule[]
  spawn?: { max_children: number; max_depth: number; targets: string[] } | null
  models?: string[]
  max_tokens?: number | null
}

export interface Agent {
  id: string
  name: string
  role: string
  parent_id: string | null
  depth: number
  status: Status
  model: string
  target: string
  host: string
  os: string
  ips: string[]
  pid: number
  started_at_ms: number
  last_seen_ms: number
  tokens_used: number
  capabilities: Capabilities
  children: Agent[]
}

export interface ActivityEvent {
  id: number
  agent_id: string
  agent_name: string
  ts_ms: number
  kind: string
  text: string
}

export interface ModelInfo {
  alias: string
  provider: string
  model: string
  host: string
  effort: string | null
  max_tokens: number | null
  has_key: boolean
}

export interface RoleEntry {
  agent_id: string
  name: string
  role: string
  model: string
  parent_id: string | null
  depth: number
  target: string
  host: string
  capabilities: Capabilities
  status: string
  instructions: string | null
  objective: string | null
  criteria: string | null
  first_seen_ms: number
  last_seen_ms: number
}

export interface Release {
  version: string
  date: string
  title: string
  summary: string
  changes: { kind: string; text: string }[]
}
