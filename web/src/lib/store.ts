import { useCallback, useEffect, useRef, useState } from 'react'
import { api, forgetToken, keepToken, savedToken, Unauthorized } from './api'
import type { ActivityEvent, Agent, ModelInfo, Release, RoleEntry } from './types'

const KEPT = 5000

export interface Cluster {
  token: string
  tree: Agent
  events: ActivityEvent[]
  offline: boolean
  models: { default: string; models: ModelInfo[] } | null
  roles: RoleEntry[] | null
  releases: { version: string; releases: Release[] } | null
  refresh: () => void
  send: (text: string) => Promise<void>
  stop: (id: string) => Promise<void>
}

/** L'état de la grappe, relu toutes les secondes et demie tant que la page est visible. */
export function useCluster() {
  const [token, setToken] = useState<string | null>(savedToken)
  const [loginError, setLoginError] = useState<string | null>(null)
  const [tree, setTree] = useState<Agent | null>(null)
  const [events, setEvents] = useState<ActivityEvent[]>([])
  const [offline, setOffline] = useState(false)
  const [models, setModels] = useState<Cluster['models']>(null)
  const [roles, setRoles] = useState<RoleEntry[] | null>(null)
  const [releases, setReleases] = useState<Cluster['releases']>(null)
  const after = useRef(0)
  const busy = useRef(false)
  const round = useRef(0)

  const logout = useCallback((message?: string) => {
    forgetToken()
    after.current = 0
    setToken(null)
    setTree(null)
    setEvents([])
    setRoles(null)
    setLoginError(message ?? null)
  }, [])

  const refresh = useCallback(async () => {
    if (!token || busy.current || document.hidden) return
    busy.current = true
    try {
      const [nextTree, log] = await Promise.all([
        api<Agent>(token, '/api/tree'),
        api<{ events: ActivityEvent[] }>(token, `/api/activity?after=${after.current}&limit=2000`),
      ])
      setOffline(false)
      setTree(nextTree)
      if (log.events.length) {
        after.current = log.events[log.events.length - 1].id
        setEvents((old) => [...old, ...log.events].slice(-KEPT))
      }
      // le répertoire des rôles bouge peu : une lecture sur six
      if (round.current++ % 6 === 0) {
        api<{ roles: RoleEntry[] }>(token, '/api/roles').then((r) => setRoles(r.roles)).catch(() => {})
      }
    } catch (error) {
      if (error instanceof Unauthorized) logout(error.message)
      else setOffline(true)
    } finally {
      busy.current = false
    }
  }, [token, logout])

  useEffect(() => {
    if (!token) return
    round.current = 0
    refresh()
    api<Cluster['models']>(token, '/api/models').then(setModels).catch(() => {})
    api<Cluster['releases']>(token, '/api/releases').then(setReleases).catch(() => {})
    const timer = setInterval(refresh, 1500)
    const onVisible = () => !document.hidden && refresh()
    document.addEventListener('visibilitychange', onVisible)
    return () => {
      clearInterval(timer)
      document.removeEventListener('visibilitychange', onVisible)
    }
  }, [token, refresh])

  const login = useCallback(async (candidate: string, remember: boolean) => {
    try {
      await api<Agent>(candidate, '/api/tree')
      keepToken(candidate, remember)
      setLoginError(null)
      setToken(candidate)
    } catch (error) {
      setLoginError(error instanceof Unauthorized ? error.message : 'Maître injoignable.')
    }
  }, [])

  const cluster: Cluster | null =
    token && tree
      ? {
          token,
          tree,
          events,
          offline,
          models,
          roles,
          releases,
          refresh,
          send: async (text) => {
            await api(token, '/api/messages', { text })
            refresh()
          },
          stop: async (id) => {
            await api(token, `/api/agents/${encodeURIComponent(id)}/stop`, {})
            refresh()
          },
        }
      : null

  return { cluster, waiting: !!token && !tree, login, loginError, logout }
}
