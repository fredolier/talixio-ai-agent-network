import { Fragment, useMemo, useState, type FormEvent } from 'react'
import { X } from 'lucide-react'
import { Connection, natureLabel, NATURES, Origin, Price, Quality, Trust } from '@/components/catalog'
import { PageHeader } from '@/components/bits'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Skeleton } from '@/components/ui/skeleton'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { count, flat, plural } from '@/lib/format'
import type { Cluster } from '@/lib/store'
import type { Agent, CatalogModel, CatalogProvider, ModelsData } from '@/lib/types'
import { cn, pick } from '@/lib/utils'

/** Le fournisseur demandé par la page Fournisseurs (`#modeles?f=mistral`). */
const wantedProvider = () => new URLSearchParams(location.hash.split('?')[1] ?? '').get('f') ?? ''

export function ModelsView({ cluster }: { cluster: Cluster }) {
  const data = cluster.models
  const [filter, setFilter] = useState('')
  const [nature, setNature] = useState('')
  const [only, setOnly] = useState(wantedProvider)
  const [active, setActive] = useState(false)
  const [europe, setEurope] = useState(false)
  const agents = useMemo(() => flat(cluster.tree), [cluster.tree])

  const groups = useMemo(() => {
    if (!data) return []
    const wanted = filter.trim().toLowerCase()
    return data.providers
      .filter((p) => (!only || p.id === only) && (!europe || p.europe === true || p.trust === 'L'))
      .map((provider) => ({
        provider,
        models: data.catalog
          .filter((m) => m.provider === provider.id)
          .filter((m) => (!nature || m.nature === nature) && (!active || m.aliases.length > 0))
          .filter((m) => !wanted || `${m.name} ${m.id} ${m.aliases.join(' ')} ${provider.name}`.toLowerCase().includes(wanted))
          .sort((a, b) => Number(b.aliases.length > 0) - Number(a.aliases.length > 0) || (b.rank ?? 0) - (a.rank ?? 0) || a.name.localeCompare(b.name, 'fr')),
      }))
      .filter((g) => g.models.length > 0)
      .sort((a, b) => Number(b.provider.connected) - Number(a.provider.connected) || a.provider.name.localeCompare(b.provider.name, 'fr'))
  }, [data, filter, nature, only, active, europe])

  if (!data) return <Skeleton className="h-64 rounded-lg" />
  const activated = data.catalog.filter((m) => m.aliases.length > 0).length
  const onlyName = data.providers.find((p) => p.id === only)?.name

  return (
    <div className="flex flex-col gap-6">
      <PageHeader
        eyebrow="Maîtrise · Modèles"
        title="Modèles"
        lead="Tous les modèles du catalogue, fournisseur par fournisseur, avec leur qualité relative et leur prix indicatif. Un modèle activé porte un alias chez le maître : c'est sous cet alias qu'il s'accorde à un agent."
      >
        <Input type="search" value={filter} onChange={(e) => setFilter(e.target.value)} placeholder="Chercher un modèle, un alias…" aria-label="Chercher un modèle" className="w-full sm:w-64" />
      </PageHeader>

      {!data.default && (
        <p role="status" className="rounded-lg border border-st-off/40 bg-card px-4 py-3 text-sm">
          <span className="font-medium">Le maître n'a pas encore de modèle.</span>{' '}
          Connectez un fournisseur dans la page <a href="#fournisseurs" className="underline underline-offset-4">Fournisseurs</a>, puis activez ici un modèle de texte : le premier activé devient le sien (un serveur local se donne par « Donner au maître »).
        </p>
      )}

      <div className="flex flex-wrap items-center gap-x-6 gap-y-3">
        <div className="flex flex-wrap gap-1" role="group" aria-label="Nature des modèles">
          {['', ...NATURES].map((n) => (
            <Button key={n} size="sm" variant={nature === n ? 'default' : 'ghost'} aria-pressed={nature === n} onClick={() => setNature(n)}>
              {n ? natureLabel(n) : 'Toutes'}
            </Button>
          ))}
        </div>
        <div className="ml-auto flex flex-wrap items-center gap-x-5 gap-y-2">
          {only && (
            <Button size="sm" variant="outline" onClick={() => { setOnly(''); history.replaceState(null, '', '#modeles') }}>
              {onlyName ?? only} <X aria-hidden /> <span className="sr-only">Retirer ce filtre</span>
            </Button>
          )}
          <Label className="font-normal">
            <Checkbox checked={active} onCheckedChange={(checked) => setActive(checked === true)} />
            Activés seulement <span className="num text-muted-foreground">{activated}</span>
          </Label>
          <Label className="font-normal">
            <Checkbox checked={europe} onCheckedChange={(checked) => setEurope(checked === true)} />
            Europe et local seulement
          </Label>
        </div>
      </div>

      <div className="rounded-lg border bg-card">
        <Table>
          <TableHeader>
            <TableRow className="hover:bg-transparent">
              <TableHead className="eyebrow px-4">Modèle</TableHead>
              <TableHead className="eyebrow">Nature</TableHead>
              <TableHead className="eyebrow">Qualité</TableHead>
              <TableHead className="eyebrow" title="Par million de tokens : entrée / sortie">Prix, entrée / sortie</TableHead>
              <TableHead className="eyebrow px-4">Chez le maître</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {groups.map(({ provider, models }) => (
              <Fragment key={provider.id}>
                <TableRow className="bg-muted/40 hover:bg-muted/40">
                  <TableCell colSpan={5} className="px-4 py-2.5">
                    <span className="flex flex-wrap items-center gap-x-4 gap-y-1">
                      <span className="font-semibold">{provider.name}</span>
                      <span className="text-[13px] text-muted-foreground">{provider.country}</span>
                      <span className="text-[13px]"><Origin provider={provider} /></span>
                      <Trust level={provider.trust} />
                      <Connection on={provider.connected} className="ml-auto text-[13px]" />
                    </span>
                  </TableCell>
                </TableRow>
                {models.map((model) => (
                  <ModelRow key={`${model.id}\u0000${model.name}`} model={model} provider={provider} data={data} agents={agents} cluster={cluster} />
                ))}
              </Fragment>
            ))}
            {groups.length === 0 && (
              <TableRow className="hover:bg-transparent">
                <TableCell colSpan={5} className="px-4 py-8 text-center text-muted-foreground">Aucun modèle ne correspond. Élargissez les filtres.</TableCell>
              </TableRow>
            )}
          </TableBody>
        </Table>
      </div>

      <p className="text-[13px] text-muted-foreground">
        Qualité : de 1 (élémentaire) à 10 (le meilleur), relative. Prix en dollars par million de tokens, indicatifs (relevé d'octobre 2026) ; « ≈ » : estimation, à vérifier chez le fournisseur.
        Pour activer un modèle, connectez d'abord son fournisseur dans la page <a href="#fournisseurs" className="underline underline-offset-4">Fournisseurs</a>. Un modèle de service se compte en appels : le nombre donné à l'activation vaut pour toute la grappe.
      </p>
    </div>
  )
}

function ModelRow({ model, provider, data, agents, cluster }: { model: CatalogModel; provider: CatalogProvider; data: ModelsData; agents: Agent[]; cluster: Cluster }) {
  const on = model.aliases.length > 0
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const run = async (action: () => Promise<unknown>) => {
    setBusy(true)
    setError(null)
    try {
      await action()
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : 'Demande non aboutie.')
    } finally {
      setBusy(false)
    }
  }
  return (
    <TableRow className={cn(!on && 'text-muted-foreground')}>
      <TableCell className="px-4 py-2.5">
        <span className={cn('block', on && 'font-medium text-foreground')}>
          {model.name}
          {model.deprecated && <span className="ml-2 text-xs font-normal text-st-off">en fin de vie</span>}
          {model.soon && <span className="ml-2 text-xs font-normal text-muted-foreground">annoncé</span>}
          {!model.listed && <span className="ml-2 text-xs font-normal text-muted-foreground">hors catalogue</span>}
        </span>
        {model.id !== model.name && <span className="num block text-xs text-muted-foreground">{model.id}</span>}
      </TableCell>
      <TableCell>{natureLabel(model.nature)}</TableCell>
      <TableCell><Quality rank={model.rank} /></TableCell>
      <TableCell className="text-[13px]"><Price model={model} /></TableCell>
      <TableCell className="px-4 whitespace-normal">
        {on ? (
          <span className="flex flex-col gap-1.5 py-0.5">
            {model.aliases.map((alias) => (
              <span key={alias} className="flex flex-wrap items-start justify-between gap-x-4 gap-y-1">
                <Activation alias={alias} data={data} agents={agents} />
                {!data.default_fixed && alias !== data.default && talks(data, alias) && (
                  <Button size="sm" variant={data.default ? 'outline' : 'default'} disabled={busy} title="Le maître réfléchira avec ce modèle" onClick={() => {
                    if (!data.default || confirm(`Donner « ${alias} » (${provider.name}) au maître ? Sa conversation en cours, résultats d'outils compris, sera envoyée à ce fournisseur.`)) run(() => cluster.change(`/api/models/${encodeURIComponent(alias)}/default`, {}))
                  }}>
                    Donner au maître
                  </Button>
                )}
                {data.activated?.includes(alias) && (
                  <Button size="sm" variant="outline" disabled={busy} onClick={() => run(() => cluster.change(`/api/models/${encodeURIComponent(alias)}/deactivate`, {}))}>
                    Désactiver
                  </Button>
                )}
              </span>
            ))}
            {error && <span role="alert" className="text-[13px] text-destructive">{error}</span>}
          </span>
        ) : !model.callable ? (
          <span title={model.soon ? 'Annoncé par le fournisseur, pas encore proposé' : "Le maître n'a pas de connecteur pour ce modèle chez ce fournisseur : il est listé pour comparer"}>Pas encore appelable</span>
        ) : provider.linked ? (
          <Activate model={model} provider={provider} agents={agents} busy={busy} error={error} run={run} cluster={cluster} />
        ) : provider.connected ? (
          <span title="Ce fournisseur est connecté par la configuration du maître : ses modèles s'y déclarent">Disponible, non activé</span>
        ) : (
          <span>Fournisseur non connecté</span>
        )}
      </TableCell>
    </TableRow>
  )
}

/** Activer un modèle d'un fournisseur connecté d'ici ; pour un modèle de service, avec son nombre d'appels. */
function Activate({ model, provider, agents, busy, error, run, cluster }: {
  model: CatalogModel; provider: CatalogProvider; agents: Agent[]; busy: boolean; error: string | null
  run: (action: () => Promise<unknown>) => Promise<void>; cluster: Cluster
}) {
  const service = model.declare && model.declare.nature !== 'llm' && !model.declare.vision ? model.declare.nature : null
  const current = service ? pick(agents.find((a) => a.depth === 0)?.capabilities.max_calls ?? {}, service) ?? 0 : 0
  const [calls, setCalls] = useState(String(current || 100))
  const submit = (event: FormEvent) => {
    event.preventDefault()
    run(() => cluster.change('/api/models/activate', { provider: provider.id, model: model.id, ...(service ? { calls: Math.max(0, Math.floor(Number(calls) || 0)) } : {}) }))
  }
  return (
    <form onSubmit={submit} className="flex flex-col gap-1.5 py-0.5">
      <span className="flex flex-wrap items-center gap-2">
        <Button type="submit" size="sm" disabled={busy}>Activer</Button>
        {service && (
          <Label className="font-normal text-[13px]">
            <Input type="number" min={0} max={1000000} value={calls} onChange={(e) => setCalls(e.target.value)} className="num h-8 w-24" aria-label={`Appels « ${service} » permis à la grappe`} />
            appels permis
          </Label>
        )}
      </span>
      {error && <span role="alert" className="text-[13px] text-destructive">{error}</span>}
    </form>
  )
}

/** Un alias du maître : ses réglages, qui s'en sert, à qui il est accordé. */
function Activation({ alias, data, agents }: { alias: string; data: ModelsData; agents: Agent[] }) {
  const info = data.models.find((m) => m.alias === alias)
  const running = agents.filter((a) => a.model === alias)
  const granted = agents.filter((a) => a.depth > 0 && a.capabilities.models?.includes(alias))
  const tokens = running.reduce((sum, a) => sum + (a.depth === 0 ? 0 : a.tokens_used), 0)
  // un modèle de service ne fait tourner aucun agent : il se compte en appels, par nature
  const service = info?.nature && info.nature !== 'llm' ? info.nature : null
  const root = agents.find((a) => a.depth === 0)
  const allowed = service ? pick(root?.capabilities.max_calls ?? {}, service) ?? 0 : 0
  const used = service ? pick(root?.calls_used ?? {}, service) ?? 0 : 0
  return (
    <span className="text-[13px] text-foreground">
      <span className="flex flex-wrap items-center gap-x-2 gap-y-1">
        <span className="size-2 rounded-full bg-st-working" aria-hidden />
        <span className="font-medium">Activé</span>
        <span className="num">{alias}</span>
        {alias === data.default && <Badge>modèle du maître</Badge>}
        {info && !info.has_key && needsKey(info.host) && <span className="text-st-off">sans clé</span>}
      </span>
      <span className="block text-muted-foreground">
        {[
          info?.effort && `effort ${info.effort}`,
          info?.max_tokens && `${count(info.max_tokens)} tokens par réponse`,
          info?.vision && 'lit les images',
          service ? (allowed > 0 ? `${count(used)} / ${count(allowed)} appels « ${service} »` : `aucun appel « ${service} » accordé (max_calls)`) : `${plural(running.length, 'agent')} en cours`,
          tokens > 0 && `${count(tokens)} tokens chez les enfants`,
        ].filter(Boolean).join(' · ')}
      </span>
      <span className="block text-muted-foreground">{granted.length ? `Accordé à ${granted.map((a) => a.name).join(', ')}.` : 'Accordé à aucun enfant.'}</span>
    </span>
  )
}

/** Ce modèle tient une conversation : le maître peut réfléchir avec lui. */
const talks = (data: ModelsData, alias: string) => {
  const nature = data.models.find((m) => m.alias === alias)?.nature
  return !nature || nature === 'llm' || nature === 'vision'
}

/** Une machine locale se passe de clé ; partout ailleurs, son absence mérite d'être dite. */
const needsKey = (host: string) => !/^(localhost|127\.0\.0\.1|\[?::1\]?)(:\d+)?$/.test(host)
