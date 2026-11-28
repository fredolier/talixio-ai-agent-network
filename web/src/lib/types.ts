// What the master's API returns. Any text coming from the cluster is displayed as text.

export type Status = 'starting' | 'idle' | 'working' | 'suspended' | 'disconnected' | 'stopped'

export interface ExecRule {
  program: string
  args?: string[] | null
  cwd?: string | null
  /** Command from a package: the hash of the installed file, checked before each launch. */
  sha256?: string | null
}

/** A role installed by the operator (package), which the master can give to an agent. */
export interface InstalledRole {
  name: string
  pack: string
  version: string
  pack_id: string
  description: string
  instructions: string
  model: string | null
  target: string | null
  exec: ExecRule[]
}

export interface Capabilities {
  fs_read?: string[]
  fs_write?: string[]
  exec?: ExecRule[]
  spawn?: { max_children: number; max_depth: number; targets: string[] } | null
  models?: string[]
  max_tokens?: number | null
  /** Calls allowed to service models, by kind; a missing kind: none. */
  max_calls?: Record<string, number>
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
  /** Completed calls to service models, by kind, descendants included. */
  calls_used?: Record<string, number>
  capabilities: Capabilities
  /** What the agent declares exceeds what its parent declares it can grant. */
  unverified?: boolean
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
  /** `llm`, `transcription`, `speech`, `image`, `embedding`, `forecast` or `decision`. */
  nature?: string
  vision?: boolean
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
  unverified?: boolean
}

export interface Release {
  version: string
  date: string
  title: string
  summary: string
  changes: { kind: string; text: string }[]
}

/** A provider from the master's catalog, and what its configuration says about it. */
export interface CatalogProvider {
  id: string
  name: string
  country: string
  /** Hosted and governed in Europe; `null` if the catalog does not know it. */
  europe: boolean | null
  /** `1` to `5`, `L` (local), or empty when not in the catalog. */
  trust: string
  kind: string
  billing: string
  /** The APIs the master can speak with it; empty: not callable yet. */
  apis: string[]
  endpoint: string
  hosts: string[]
  /** One of its models is declared on the master, with its key. */
  connected: boolean
  configured: number
  listed: boolean
  /** Connected from this interface: its models are enabled and it is disconnected from here. */
  linked?: boolean
  /** What the page offers: a key to paste, an authorization in the browser, nothing for a local machine, or the configuration. */
  connect?: 'key' | 'oauth' | 'local' | 'cli' | 'config'
  /** The provider's page where a key is created. */
  keys_url?: string
  /** The provider also answers without a key, with low limits. */
  keyless?: boolean
}

export interface CatalogModel {
  name: string
  id: string
  provider: string
  nature: string
  /** Relative quality, from 1 to 10; `null` when not in the catalog. */
  rank: number | null
  price_in: number | null
  price_out: number | null
  unit_price: number | null
  unit: string | null
  currency: string | null
  estimated: boolean
  deprecated: boolean
  /** Announced, not offered yet. */
  soon: boolean
  /** The aliases under which the master has enabled it. */
  aliases: string[]
  /** The master has a connector for it: an agent can call it once the model is enabled. */
  callable: boolean
  /** How to declare it in the master's configuration. */
  declare?: { provider: string; nature: string; vision: boolean }
  listed: boolean
}

export interface ModelsData {
  /** The alias of the master's model; empty: it has none yet. */
  default: string
  /** The master's model comes from its configuration: it cannot be changed from here. */
  default_fixed?: boolean
  models: ModelInfo[]
  providers: CatalogProvider[]
  catalog: CatalogModel[]
  /** The aliases enabled from the interface: only these can be disabled from here. */
  activated?: string[]
}

/** A scheduled task: an instruction the master receives on its own, at a fixed time. */
export interface Task {
  id: string
  name: string
  text: string
  kind: 'hours' | 'daily' | 'weekly'
  hours?: number
  /** Minutes after midnight, in the local time of whoever created it. */
  minute?: number
  /** 0: Monday … 6: Sunday. */
  weekday?: number
  tz_offset_min: number
  enabled: boolean
  next_run_ms: number
  last_run_ms: number | null
  created_ms: number
}

export interface TasksData {
  tasks: Task[]
  now_ms: number
  max: number
}

/** A secret for a command, as the master shows it: never its value. */
export interface Secret {
  name: string
  enabled: boolean
  command: string
  variable: string
  roles: string[]
  master: boolean
  remote: boolean
  env: Record<string, string>
  updated_ms: number
  used_ms: number
  used_by: string
  /** The command's program is no longer the one this secret was saved for. */
  stale: boolean
}

/** A command of an installed pack that accepts secrets. */
export interface SecretCommand {
  name: string
  secrets: string[]
  env: string[]
  roles: string[]
}

export interface SecretsData {
  secrets: Secret[]
  commands: SecretCommand[]
}
