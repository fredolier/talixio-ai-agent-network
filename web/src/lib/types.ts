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
  /** Appels permis aux modèles de service, par nature ; une nature absente : aucun. */
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
  /** Appels aboutis aux modèles de service, par nature, descendance comprise. */
  calls_used?: Record<string, number>
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
  /** `llm`, `transcription`, `speech`, `image`, `embedding`, `forecast` ou `decision`. */
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
  /** Les API que le maître sait parler chez lui ; vide : pas encore appelable. */
  apis: string[]
  endpoint: string
  hosts: string[]
  /** Un de ses modèles est déclaré chez le maître, avec sa clé. */
  connected: boolean
  configured: number
  listed: boolean
  /** Connecté depuis cette interface : ses modèles s'activent et il se déconnecte d'ici. */
  linked?: boolean
  /** Ce que la page propose : une clé à coller, une autorisation dans le navigateur, rien pour une machine locale, ou la configuration. */
  connect?: 'key' | 'oauth' | 'local' | 'cli' | 'config'
  /** La page du fournisseur où l'on crée une clé. */
  keys_url?: string
  /** Le fournisseur répond aussi sans clé, avec des limites basses. */
  keyless?: boolean
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
  /** Annoncé, pas encore proposé. */
  soon: boolean
  /** Les alias sous lesquels le maître l'a activé. */
  aliases: string[]
  /** Le maître a un connecteur pour lui : un agent peut l'appeler une fois le modèle activé. */
  callable: boolean
  /** Comment le déclarer dans la configuration du maître. */
  declare?: { provider: string; nature: string; vision: boolean }
  listed: boolean
}

export interface ModelsData {
  /** L'alias du modèle du maître ; vide : il n'en a pas encore. */
  default: string
  /** Le modèle du maître vient de sa configuration : il ne se change pas d'ici. */
  default_fixed?: boolean
  models: ModelInfo[]
  providers: CatalogProvider[]
  catalog: CatalogModel[]
  /** Les alias activés depuis l'interface : eux seuls se désactivent d'ici. */
  activated?: string[]
}

/** Une tâche planifiée : une consigne que le maître reçoit seul, à heure fixe. */
export interface Task {
  id: string
  name: string
  text: string
  kind: 'hours' | 'daily' | 'weekly'
  hours?: number
  /** Minutes après minuit, à l'heure locale de qui l'a créée. */
  minute?: number
  /** 0 : lundi … 6 : dimanche. */
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
