import { Fragment, useMemo, useState, type FormEvent } from 'react'
import { X } from 'lucide-react'
import { Connection, Mixed, natureLabel, NATURES, Origin, Price, Quality, Trust } from '@/components/catalog'
import { PageHeader } from '@/components/bits'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Skeleton } from '@/components/ui/skeleton'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { count, flat } from '@/lib/format'
import { t, tn } from '@/lib/i18n'
import type { Cluster } from '@/lib/store'
import type { Agent, CatalogModel, CatalogProvider, ModelsData } from '@/lib/types'
import { cn, pick } from '@/lib/utils'

/** The provider asked for by the Providers page (`#modeles?f=mistral`). */
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
        eyebrow={t('Control · Models')}
        title={t('Models')}
        lead={t('Every model in the catalog, provider by provider, with its relative quality and indicative price. An enabled model has an alias on the master: it is granted to an agent under that alias.')}
      >
        <Input type="search" value={filter} onChange={(e) => setFilter(e.target.value)} placeholder={t('Search for a model, an alias…')} aria-label={t('Search for a model')} className="w-full sm:w-64" />
      </PageHeader>

      {!data.default && (
        <p role="status" className="rounded-lg border border-st-off/40 bg-card px-4 py-3 text-sm">
          <span className="font-medium">{t('The master has no model yet.')}</span>{' '}
          <Mixed
            text={t('Connect a provider on the {providers} page, then enable a text model here: the first one enabled becomes its own (a local server is given with “Give to the master”).')}
            slots={{ providers: <a href="#fournisseurs" className="underline underline-offset-4">{t('Providers')}</a> }}
          />
        </p>
      )}

      <div className="flex flex-wrap items-center gap-x-6 gap-y-3">
        <div className="flex flex-wrap gap-1" role="group" aria-label={t('Model kind')}>
          {['', ...NATURES].map((n) => (
            <Button key={n} size="sm" variant={nature === n ? 'default' : 'ghost'} aria-pressed={nature === n} onClick={() => setNature(n)}>
              {n ? natureLabel(n) : t('All kinds')}
            </Button>
          ))}
        </div>
        <div className="ml-auto flex flex-wrap items-center gap-x-5 gap-y-2">
          {only && (
            <Button size="sm" variant="outline" onClick={() => { setOnly(''); history.replaceState(null, '', '#modeles') }}>
              {onlyName ?? only} <X aria-hidden /> <span className="sr-only">{t('Remove this filter')}</span>
            </Button>
          )}
          <Label className="font-normal">
            <Checkbox checked={active} onCheckedChange={(checked) => setActive(checked === true)} />
            {t('Enabled only')} <span className="num text-muted-foreground">{activated}</span>
          </Label>
          <Label className="font-normal">
            <Checkbox checked={europe} onCheckedChange={(checked) => setEurope(checked === true)} />
            {t('Europe and local only')}
          </Label>
        </div>
      </div>

      <div className="rounded-lg border bg-card">
        <Table>
          <TableHeader>
            <TableRow className="hover:bg-transparent">
              <TableHead className="eyebrow px-4">{t('Model')}</TableHead>
              <TableHead className="eyebrow">{t('Kind')}</TableHead>
              <TableHead className="eyebrow">{t('Quality')}</TableHead>
              <TableHead className="eyebrow" title={t('Per million tokens: input / output')}>{t('Price, input / output')}</TableHead>
              <TableHead className="eyebrow px-4">{t('On the master')}</TableHead>
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
                <TableCell colSpan={5} className="px-4 py-8 text-center text-muted-foreground">{t('No model matches. Widen the filters.')}</TableCell>
              </TableRow>
            )}
          </TableBody>
        </Table>
      </div>

      <p className="text-[13px] text-muted-foreground">
        {t('Quality: from 1 (basic) to 10 (the best), relative. Prices in dollars per million tokens, indicative (recorded in October 2026); “≈”: an estimate, to check with the provider.')}{' '}
        <Mixed
          text={t('To enable a model, first connect its provider on the {providers} page. A service model is counted in calls: the number given when enabling applies to the whole cluster.')}
          slots={{ providers: <a href="#fournisseurs" className="underline underline-offset-4">{t('Providers')}</a> }}
        />
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
      setError(failure instanceof Error ? failure.message : t('The request did not complete.'))
    } finally {
      setBusy(false)
    }
  }
  return (
    <TableRow className={cn(!on && 'text-muted-foreground')}>
      <TableCell className="px-4 py-2.5">
        <span className={cn('block', on && 'font-medium text-foreground')}>
          {model.name}
          {model.deprecated && <span className="ml-2 text-xs font-normal text-st-off">{t('end of life')}</span>}
          {model.soon && <span className="ml-2 text-xs font-normal text-muted-foreground">{t('announced')}</span>}
          {!model.listed && <span className="ml-2 text-xs font-normal text-muted-foreground">{t('not in the catalog')}</span>}
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
                  <Button size="sm" variant={data.default ? 'outline' : 'default'} disabled={busy} title={t('The master will think with this model')} onClick={() => {
                    if (!data.default || confirm(t('Give “{alias}” ({provider}) to the master? Its current conversation, tool results included, will be sent to this provider.', { alias, provider: provider.name }))) run(() => cluster.change(`/api/models/${encodeURIComponent(alias)}/default`, {}))
                  }}>
                    {t('Give to the master')}
                  </Button>
                )}
                {data.activated?.includes(alias) && (
                  <Button size="sm" variant="outline" disabled={busy} onClick={() => run(() => cluster.change(`/api/models/${encodeURIComponent(alias)}/deactivate`, {}))}>
                    {t('Disable')}
                  </Button>
                )}
              </span>
            ))}
            {error && <span role="alert" className="text-[13px] text-destructive">{error}</span>}
          </span>
        ) : !model.callable ? (
          <span title={model.soon ? t('Announced by the provider, not offered yet') : t('The master has no connector for this model at this provider: it is listed for comparison')}>{t('Not callable yet')}</span>
        ) : provider.linked ? (
          <Activate model={model} provider={provider} agents={agents} busy={busy} error={error} run={run} cluster={cluster} />
        ) : provider.connected ? (
          <span title={t("This provider is connected through the master's configuration: its models are declared there")}>{t('Available, not enabled')}</span>
        ) : (
          <span>{t('Provider not connected')}</span>
        )}
      </TableCell>
    </TableRow>
  )
}

/** Enable a model of a provider connected from here; for a service model, with its number of calls. */
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
        <Button type="submit" size="sm" disabled={busy}>{t('Enable')}</Button>
        {service && (
          <Label className="font-normal text-[13px]">
            <Input type="number" min={0} max={1000000} value={calls} onChange={(e) => setCalls(e.target.value)} className="num h-8 w-24" aria-label={t('“{service}” calls allowed to the cluster', { service })} />
            {t('calls allowed')}
          </Label>
        )}
      </span>
      {error && <span role="alert" className="text-[13px] text-destructive">{error}</span>}
    </form>
  )
}

/** An alias of the master: its settings, who uses it, whom it is granted to. */
function Activation({ alias, data, agents }: { alias: string; data: ModelsData; agents: Agent[] }) {
  const info = data.models.find((m) => m.alias === alias)
  const running = agents.filter((a) => a.model === alias)
  const granted = agents.filter((a) => a.depth > 0 && a.capabilities.models?.includes(alias))
  const tokens = running.reduce((sum, a) => sum + (a.depth === 0 ? 0 : a.tokens_used), 0)
  // a service model runs no agent: it is counted in calls, by kind
  const service = info?.nature && info.nature !== 'llm' ? info.nature : null
  const root = agents.find((a) => a.depth === 0)
  const allowed = service ? pick(root?.capabilities.max_calls ?? {}, service) ?? 0 : 0
  const used = service ? pick(root?.calls_used ?? {}, service) ?? 0 : 0
  return (
    <span className="text-[13px] text-foreground">
      <span className="flex flex-wrap items-center gap-x-2 gap-y-1">
        <span className="size-2 rounded-full bg-st-working" aria-hidden />
        <span className="font-medium">{t('Enabled')}</span>
        <span className="num">{alias}</span>
        {alias === data.default && <Badge>{t("master's model")}</Badge>}
        {info && !info.has_key && needsKey(info.host) && <span className="text-st-off">{t('no key')}</span>}
      </span>
      <span className="block text-muted-foreground">
        {[
          info?.effort && t('effort {effort}', { effort: info.effort }),
          info?.max_tokens && t('{count} tokens per reply', { count: count(info.max_tokens) }),
          info?.vision && t('reads images'),
          service
            ? allowed > 0
              ? t('{used} / {allowed} calls to “{service}”', { used: count(used), allowed: count(allowed), service })
              : t('no “{service}” call granted (max_calls)', { service })
            : tn(running.length, '{n} agent running', '{n} agents running'),
          tokens > 0 && t('{count} tokens used by children', { count: count(tokens) }),
        ].filter(Boolean).join(' · ')}
      </span>
      <span className="block text-muted-foreground">{granted.length ? t('Granted to {names}.', { names: granted.map((a) => a.name).join(', ') }) : t('Granted to no child.')}</span>
    </span>
  )
}

/** This model holds a conversation: the master can think with it. */
const talks = (data: ModelsData, alias: string) => {
  const nature = data.models.find((m) => m.alias === alias)?.nature
  return !nature || nature === 'llm' || nature === 'vision'
}

/** A local machine needs no key; anywhere else, its absence is worth saying. */
const needsKey = (host: string) => !/^(localhost|127\.0\.0\.1|\[?::1\]?)(:\d+)?$/.test(host) && !host.endsWith('.invalid')
