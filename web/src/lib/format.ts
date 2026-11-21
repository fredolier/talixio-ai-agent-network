import type { Agent, ExecRule, Status } from './types'

export const STATUS: Record<Status, string> = {
  starting: 'démarre',
  idle: 'disponible',
  working: 'au travail',
  suspended: 'en veille',
  disconnected: 'injoignable',
  stopped: 'arrêté',
}

export const KIND: Record<string, string> = {
  status: 'état',
  input: 'reçu',
  tool: 'outil',
  tool_error: 'refus',
  spawn: 'création',
  report: 'rapport',
  message: 'message',
  error: 'erreur',
  link: 'lien',
  autre: 'autre',
}

export const flat = (node: Agent, out: Agent[] = []): Agent[] => {
  out.push(node)
  node.children.forEach((child) => flat(child, out))
  return out
}

export const count = (n: number) => n.toLocaleString('fr-FR')
export const plural = (n: number, word: string) => `${count(n)} ${word}${n > 1 ? 's' : ''}`
export const hour = (ms: number) =>
  new Date(ms).toLocaleTimeString('fr-FR', { hour: '2-digit', minute: '2-digit', second: '2-digit' })
export const day = (value: number | string) =>
  new Date(value).toLocaleDateString('fr-FR', { day: 'numeric', month: 'long', year: 'numeric' })
export const moment = (ms: number) =>
  new Date(ms).toLocaleString('fr-FR', { day: 'numeric', month: 'short', hour: '2-digit', minute: '2-digit' })

export const targetLabel = (target: string) => (target === 'root' ? 'maître' : target === 'local' ? 'même machine' : target)

/** Deux règles disent-elles la même chose ? */
export const sameRule = (a: ExecRule, b: ExecRule) =>
  a.program === b.program && (a.cwd ?? null) === (b.cwd ?? null) && JSON.stringify(a.args ?? null) === JSON.stringify(b.args ?? null)
