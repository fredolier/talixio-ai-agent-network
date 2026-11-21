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
  /** Ce que l'agent déclare dépasse ce que son parent déclare pouvoir accorder. */
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

/** Un fournisseur du catalogue du maître, et ce que sa configuration en dit. */
export interface CatalogProvider {
  id: string
  name: string
  country: string
  /** Hébergé et régi en Europe ; `null` si le catalogue ne le connaît pas. */
  europe: boolean | null
  /** `1` à `5`, `L` (local), ou vide hors catalogue. */
  trust: string
  kind: string
  billing: string
  /** Comment le maître sait l'appeler ; vide : pas encore appelable. */
  api: string
  endpoint: string
  hosts: string[]
  /** Un de ses modèles est déclaré chez le maître, avec sa clé. */
  connected: boolean
  configured: number
  listed: boolean
}

export interface CatalogModel {
  name: string
  id: string
  provider: string
  nature: string
  /** Qualité relative, de 1 à 10 ; `null` hors catalogue. */
  rank: number | null
  price_in: number | null
  price_out: number | null
  unit_price: number | null
  unit: string | null
  currency: string | null
  estimated: boolean
  deprecated: boolean
  /** Les alias sous lesquels le maître l'a activé. */
  aliases: string[]
  /** Un agent peut l'appeler (texte et vision ; pas encore l'image ni la voix). */
  callable: boolean
  listed: boolean
}

export interface ModelsData {
  default: string
  models: ModelInfo[]
  providers: CatalogProvider[]
  catalog: CatalogModel[]
}
