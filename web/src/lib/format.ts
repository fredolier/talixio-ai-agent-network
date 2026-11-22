import { currentLang, t, tx } from './i18n'
import type { Agent, ExecRule } from './types'
import { pick } from './utils'

const STATUS: Record<string, string> = {
  starting: tx('starting'),
  idle: tx('available'),
  working: tx('working'),
  suspended: tx('on standby'),
  disconnected: tx('unreachable'),
  stopped: tx('stopped'),
}

const KIND: Record<string, string> = {
  status: tx('status'),
  input: tx('received'),
  tool: tx('tool'),
  tool_error: tx('denied'),
  spawn: tx('creation'),
  report: tx('report'),
  message: tx('message'),
  error: tx('error'),
  link: tx('link'),
  autre: tx('other'),
}

/** The word for an agent status, in the current language; an unknown status is shown as received. */
export const statusLabel = (status: unknown) => {
  const known = pick(STATUS, status)
  return known ? t(known) : String(status)
}

/** The word for a kind of journal event, in the current language. */
export const kindLabel = (kind: unknown) => {
  const known = pick(KIND, kind)
  return known ? t(known) : String(kind)
}

export const flat = (node: Agent, out: Agent[] = []): Agent[] => {
  out.push(node)
  node.children.forEach((child) => flat(child, out))
  return out
}

export const count = (n: number) => n.toLocaleString(currentLang())
export const hour = (ms: number) =>
  new Date(ms).toLocaleTimeString(currentLang(), { hour: '2-digit', minute: '2-digit', second: '2-digit' })
export const day = (value: number | string) =>
  new Date(value).toLocaleDateString(currentLang(), { day: 'numeric', month: 'long', year: 'numeric' })
export const moment = (ms: number) =>
  new Date(ms).toLocaleString(currentLang(), { day: 'numeric', month: 'short', hour: '2-digit', minute: '2-digit' })

export const targetLabel = (target: string) => (target === 'root' ? t('master') : target === 'local' ? t('same machine') : target)

/** Do two rules say the same thing? */
export const sameRule = (a: ExecRule, b: ExecRule) =>
  a.program === b.program && (a.cwd ?? null) === (b.cwd ?? null) && JSON.stringify(a.args ?? null) === JSON.stringify(b.args ?? null)
