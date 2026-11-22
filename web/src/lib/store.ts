import { useCallback, useEffect, useRef, useState } from 'react'
import { api, forgetToken, keepToken, savedToken, Unauthorized } from './api'
import type { ActivityEvent, Agent, InstalledRole, ModelsData, Release, RoleEntry } from './types'

const KEPT = 5000
/** Le fournisseur dont l'autorisation dans le navigateur est en cours, le temps de l'aller-retour. */
const OAUTH = 'talixio-oauth'

export interface Cluster {
  token: string
  tree: Agent
  events: ActivityEvent[]
  offline: boolean
  models: ModelsData | null
  roles: RoleEntry[] | null
  installedRoles: InstalledRole[]
  releases: { version: string; releases: Release[] } | null
  refresh: () => void
  /** Ce que le retour d'une autorisation dans le navigateur a donné. */
  notice: string | null
  /** Demande un changement au maître (connexion, activation…), puis relit les modèles. */
  change: <T>(path: string, body: unknown) => Promise<T>
  /** Part chez le fournisseur pour autoriser le maître ; le retour se fait sur cette page. */
  authorize: (provider: string) => Promise<void>
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
  const [installedRoles, setInstalledRoles] = useState<InstalledRole[]>([])
  const [releases, setReleases] = useState<Cluster['releases']>(null)
  const [notice, setNotice] = useState<string | null>(null)
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
        api<{ roles: RoleEntry[]; installed?: InstalledRole[] }>(token, '/api/roles')
          .then((r) => {
            setRoles(r.roles)
            setInstalledRoles(r.installed ?? [])
          })
          .catch(() => {})
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
    const reload = () => api<Cluster['models']>(token, '/api/models').then(setModels).catch(() => {})
    // retour d'une autorisation commencée ici : le code part chez le maître et quitte l'adresse
    const code = new URLSearchParams(location.search).get('code')
    // la marque ne vaut que dix minutes, comme l'autorisation chez le maître
    const [pending, since] = (sessionStorage.getItem(OAUTH) ?? '').split(' ')
    const expired = !!pending && Date.now() - Number(since) > 600_000
    if (expired) sessionStorage.removeItem(OAUTH)
    if (code && pending && !expired) {
      sessionStorage.removeItem(OAUTH)
      history.replaceState(null, '', `${location.pathname}#fournisseurs`)
      api(token, `/api/providers/${encodeURIComponent(pending)}/oauth/finish`, { code })
        .then(() => setNotice('Fournisseur connecté. Vous pouvez activer ses modèles.'))
        .catch((error: Error) => setNotice(`Autorisation non aboutie : ${error.message}`))
        .finally(reload)
    } else reload()
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
          installedRoles,
          releases,
          refresh,
          notice,
          change: async <T,>(path: string, body: unknown) => {
            setNotice(null)
            try {
              return await api<T>(token, path, body)
            } finally {
              api<Cluster['models']>(token, '/api/models').then(setModels).catch(() => {})
              refresh()
            }
          },
          authorize: async (provider) => {
            const { url } = await api<{ url: string }>(token, `/api/providers/${encodeURIComponent(provider)}/oauth`, { callback: `${location.origin}/` })
            sessionStorage.setItem(OAUTH, `${provider} ${Date.now()}`)
            location.assign(url)
          },
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
