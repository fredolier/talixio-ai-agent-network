import { useState, type FormEvent } from 'react'
import { Connection, Mixed, Origin, Trust, TRUST_LEVELS } from '@/components/catalog'
import { PageHeader } from '@/components/bits'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Skeleton } from '@/components/ui/skeleton'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { t, tn } from '@/lib/i18n'
import type { Cluster } from '@/lib/store'
import type { CatalogProvider, ModelsData } from '@/lib/types'
import { cn } from '@/lib/utils'

export function ProvidersView({ cluster }: { cluster: Cluster }) {
  const data = cluster.models
  const [europe, setEurope] = useState(false)
  const [connected, setConnected] = useState(false)
  const [open, setOpen] = useState<string | null>(null)
  if (!data) return <Skeleton className="h-64 rounded-lg" />

  // connected first, then European, then by name: what is in use is at the top
  const rows = data.providers
    .filter((p) => (!europe || p.europe === true || p.trust === 'L') && (!connected || p.connected))
    .sort((a, b) => Number(b.connected) - Number(a.connected) || Number(b.europe === true) - Number(a.europe === true) || a.name.localeCompare(b.name, 'fr'))
  const selected = rows.find((p) => p.id === open)
  const total = data.providers.length
  const on = data.providers.filter((p) => p.connected).length
  const european = data.providers.filter((p) => p.europe === true).length

  return (
    <div className="flex flex-col gap-6">
      <PageHeader
        eyebrow={t('Control · Providers')}
        title={t('Providers')}
        lead={t('Every provider the master knows, wherever they are. Choose one to connect to it from here, with your key. The key stays on the master: no agent receives it, and this page never reads it back.')}
      />
      <div className="flex flex-wrap items-center gap-x-6 gap-y-3">
        <p className="text-[13px] text-muted-foreground">
          <Mixed
            text={tn(on, '{n} provider connected out of {total}', '{n} providers connected out of {total}', { n: '{n}' })}
            slots={{ n: <span className="num font-medium text-foreground">{on}</span>, total: <span className="num">{total}</span> }}
          />{' '}
          ·{' '}
          <Mixed text={tn(european, '{n} is European', '{n} are European', { n: '{n}' })} slots={{ n: <span className="num">{european}</span> }} />
        </p>
        <div className="ml-auto flex flex-wrap gap-x-5 gap-y-2">
          <Label className="font-normal">
            <Checkbox checked={europe} onCheckedChange={(checked) => setEurope(checked === true)} />
            {t('Europe and local only')}
          </Label>
          <Label className="font-normal">
            <Checkbox checked={connected} onCheckedChange={(checked) => setConnected(checked === true)} />
            {t('Connected only')}
          </Label>
        </div>
      </div>

      <div className="rounded-lg border bg-card">
        <Table>
          <TableHeader>
            <TableRow className="hover:bg-transparent">
              <TableHead className="eyebrow px-4">{t('Provider')}</TableHead>
              <TableHead className="eyebrow">{t('Country')}</TableHead>
              <TableHead className="eyebrow">{t('Origin')}</TableHead>
              <TableHead className="eyebrow">{t('Trust')}</TableHead>
              <TableHead className="eyebrow">{t('Billing')}</TableHead>
              <TableHead className="eyebrow text-right">{t('Models')}</TableHead>
              <TableHead className="eyebrow px-4">{t('Status')}</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {rows.map((provider) => {
              const known = data.catalog.filter((m) => m.provider === provider.id)
              const active = known.filter((m) => m.aliases.length > 0).length
              return (
                <TableRow
                  key={provider.id}
                  data-state={open === provider.id ? 'selected' : undefined}
                  className="cursor-pointer"
                  onClick={() => setOpen(open === provider.id ? null : provider.id)}
                >
                  <TableCell className="px-4 py-3">
                    <button type="button" aria-expanded={open === provider.id} className="text-left font-medium outline-none focus-visible:underline">
                      {provider.name}
                    </button>
                    {!provider.listed && <span className="ml-2 text-xs text-muted-foreground">{t('not in the catalog')}</span>}
                  </TableCell>
                  <TableCell>{provider.country || <span className="text-muted-foreground">{t('not specified')}</span>}</TableCell>
                  <TableCell><Origin provider={provider} /></TableCell>
                  <TableCell><Trust level={provider.trust} withLabel /></TableCell>
                  <TableCell className="text-muted-foreground">{provider.billing === 'usage' ? t('pay as you go') : provider.billing || '—'}</TableCell>
                  <TableCell className="text-right">
                    <span className="num">{known.length}</span>
                    {active > 0 && (
                      <span className="text-muted-foreground">
                        {' · '}
                        <Mixed text={tn(active, '{n} model enabled', '{n} models enabled', { n: '{n}' })} slots={{ n: <span className="num text-foreground">{active}</span> }} />
                      </span>
                    )}
                  </TableCell>
                  <TableCell className="px-4"><Connection on={provider.connected} /></TableCell>
                </TableRow>
              )
            })}
            {rows.length === 0 && (
              <TableRow className="hover:bg-transparent">
                <TableCell colSpan={7} className="px-4 py-8 text-center text-muted-foreground">{t('No provider matches these filters.')}</TableCell>
              </TableRow>
            )}
          </TableBody>
        </Table>
      </div>

      {cluster.notice && <p role="status" className="rounded-md border bg-card px-4 py-3 text-[13px]">{cluster.notice}</p>}

      {selected ? <ProviderDetail key={selected.id} provider={selected} data={data} cluster={cluster} /> : <p className="text-[13px] text-muted-foreground">{t('Choose a provider to see how to connect to it.')}</p>}

      <p className="flex flex-wrap items-center gap-x-4 gap-y-2 border-t pt-4 text-[13px] text-muted-foreground">
        <span className="eyebrow">{t('Trust')}</span>
        {TRUST_LEVELS.map(({ level, label }) => (
          <span key={level} className="inline-flex items-center gap-1.5"><Trust level={level} /> {t(label)}</span>
        ))}
      </p>
    </div>
  )
}

function ProviderDetail({ provider, data, cluster }: { provider: CatalogProvider; data: ModelsData; cluster: Cluster }) {
  const models = data.catalog.filter((m) => m.provider === provider.id)
  const active = models.filter((m) => m.aliases.length > 0)
  // the catalog does not always know the exact name the API expects: never make one up
  const example = models.find((m) => m.callable && !m.deprecated && /^[a-z0-9][a-z0-9._:/-]*$/.test(m.id))
  const local = provider.trust === 'L'
  const [key, setKey] = useState('')
  const [other, setOther] = useState('')
  const [busy, setBusy] = useState(false)
  const [message, setMessage] = useState<{ ok: boolean; text: string } | null>(null)
  const path = `/api/providers/${encodeURIComponent(provider.id)}`
  const run = async (action: () => Promise<string>) => {
    setBusy(true)
    setMessage(null)
    try {
      setMessage({ ok: true, text: await action() })
    } catch (error) {
      setMessage({ ok: false, text: error instanceof Error ? error.message : t('The request did not complete.') })
    } finally {
      setBusy(false)
    }
  }
  const connect = (event: FormEvent) => {
    event.preventDefault()
    run(async () => {
      const done = await cluster.change<{ verified: boolean }>(`${path}/connect`, { key })
      setKey('')
      return done.verified ? t('Connected: the provider accepted the key.') : t('Connected. The provider did not allow the key to be checked: it will be at the first call.')
    })
  }
  const disconnect = () => {
    if (!confirm(t('Disconnect {provider}? Its key is erased from the master and its models enabled from here are removed.', { provider: provider.name }))) return
    run(async () => {
      await cluster.change(`${path}/disconnect`, {})
      return t('Disconnected: the key is erased.')
    })
  }
  const activate = (event: FormEvent) => {
    event.preventDefault()
    run(async () => {
      const done = await cluster.change<{ alias: string }>('/api/models/activate', { provider: provider.id, model: other.trim() })
      setOther('')
      return t('Model enabled under the alias “{alias}”.', { alias: done.alias })
    })
  }
  const feedback = message && <p role={message.ok ? 'status' : 'alert'} className={cn('text-[13px]', !message.ok && 'text-destructive')}>{message.text}</p>
  return (
    <section className="rounded-lg border bg-card p-5">
      <div className="flex flex-wrap items-baseline gap-x-4 gap-y-1">
        <h2 className="text-base font-semibold">{provider.name}</h2>
        <span className="text-[13px] text-muted-foreground">{provider.kind}</span>
        <a href={`#modeles?f=${encodeURIComponent(provider.id)}`} className="ml-auto text-[13px] underline underline-offset-4">
          {tn(models.length, 'See its {n} model', 'See its {n} models')}
        </a>
      </div>
      {provider.linked ? (
        <div className="mt-3 flex flex-col gap-3 text-[13px]">
          <p>
            {t('Connected from this page.')} {active.length > 0 ? t('Enabled on the master:') : t('No model enabled yet:')}{' '}
            {active.map((m, index) => (
              <span key={m.id}>
                {index > 0 && ', '}
                {m.name} <span className="text-muted-foreground"><Mixed text={t('(alias {aliases})')} slots={{ aliases: <span className="num">{m.aliases.join(', ')}</span> }} /></span>
              </span>
            ))}
            {active.length === 0 && <a href={`#modeles?f=${encodeURIComponent(provider.id)}`} className="underline underline-offset-4">{t('enable one on the Models page')}</a>}
            .
          </p>
          <form onSubmit={activate} className="flex flex-wrap items-end gap-3">
            <div className="flex min-w-64 flex-1 flex-col gap-1.5">
              <Label htmlFor="other-model">{t('A model missing from the catalog')}</Label>
              <Input id="other-model" value={other} onChange={(e) => setOther(e.target.value)} placeholder={t('its exact name at the provider')} className="font-mono" autoComplete="off" spellCheck={false} />
            </div>
            <Button type="submit" variant="outline" disabled={busy || !other.trim()}>{t('Enable')}</Button>
            <Button type="button" variant="outline" disabled={busy} onClick={disconnect} className="ml-auto">{t('Disconnect')}</Button>
          </form>
          {feedback}
        </div>
      ) : provider.connect && provider.connect !== 'config' && !provider.connected ? (
        <div className="mt-3 flex flex-col gap-3 text-[13px]">
          {provider.connect === 'cli' ? (
            <>
              <p>
                <Mixed text={t("Your Claude subscription (Pro, Max), through the official `claude` command installed on the master machine. The master runs it as is and sees neither password nor token: the command signs in by itself. Sign it in first (`claude` then `/login`, or the `CLAUDE_CODE_OAUTH_TOKEN` variable taken from `claude setup-token`). Usage counts against the limits of the subscription, under Anthropic's terms for personal use.")} />
              </p>
              <div><Button disabled={busy} onClick={() => run(async () => { await cluster.change(`${path}/connect`, { key: '' }); return t('claude command found.') })}>{t('Connect')}</Button></div>
            </>
          ) : provider.connect === 'local' ? (
            <>
              <p><Mixed text={t('A local machine needs no key. The master checks that a model server answers at {endpoint}.')} slots={{ endpoint: <span className="num">{provider.endpoint}</span> }} /></p>
              <div><Button disabled={busy} onClick={() => run(async () => { await cluster.change(`${path}/connect`, { key: '' }); return t('Connected.') })}>{t('Connect')}</Button></div>
            </>
          ) : (
            <>
              {provider.connect === 'oauth' && (
                <div className="flex flex-wrap items-center gap-3">
                  <Button disabled={busy} onClick={() => run(async () => { await cluster.authorize(provider.id); return t("Opening the provider's page…") })}>
                    {t('Sign in with {provider}', { provider: provider.name })}
                  </Button>
                  <span className="text-muted-foreground">{t('You authorize the master at the provider, without copying a key. Otherwise, paste a key:')}</span>
                </div>
              )}
              {provider.keyless && (
                <div className="flex flex-wrap items-center gap-3">
                  <Button disabled={busy} onClick={() => run(async () => { await cluster.change(`${path}/connect`, { key: '' }); return t('Connected without a key.') })}>
                    {t('Connect without a key')}
                  </Button>
                  <span className="text-muted-foreground">{t('{provider} also answers without an account, with low limits: enough to try it. For regular use, paste a key:', { provider: provider.name })}</span>
                </div>
              )}
              <form onSubmit={connect} className="flex flex-wrap items-end gap-3">
                <div className="flex min-w-64 flex-1 flex-col gap-1.5">
                  <Label htmlFor="provider-key">{t('{provider} API key', { provider: provider.name })}</Label>
                  <Input id="provider-key" type="password" value={key} onChange={(e) => setKey(e.target.value)} className="font-mono" autoComplete="off" spellCheck={false} />
                </div>
                <Button type="submit" variant={provider.connect === 'oauth' || provider.keyless ? 'outline' : 'default'} disabled={busy || key.trim().length < 8}>{t('Connect')}</Button>
                {provider.keys_url && (
                  <a href={provider.keys_url} target="_blank" rel="noopener noreferrer" className="pb-2 underline underline-offset-4">{t('Get a key')}</a>
                )}
              </form>
              <p className="text-muted-foreground">
                {t('The master tries the key at the provider, then stores it in a file that only its account can read. It is never displayed again, nor given to an agent.')}
              </p>
            </>
          )}
          {feedback}
        </div>
      ) : provider.connected ? (
        <p className="mt-3 text-[13px]">
          {t("Connected through the master's configuration.")} {active.length > 0 ? `${t('Enabled:')} ` : ''}
          {active.map((m, index) => (
            <span key={m.id}>
              {index > 0 && ', '}
              {m.name} <span className="text-muted-foreground"><Mixed text={t('(alias {aliases})')} slots={{ aliases: <span className="num">{m.aliases.join(', ')}</span> }} /></span>
            </span>
          ))}
        </p>
      ) : !provider.apis?.length || !example?.declare ? (
        <p className="mt-3 text-[13px] text-muted-foreground">
          {t('The master has no connector yet for the models of this provider. It is listed for comparison.')}
        </p>
      ) : (
        <div className="mt-3 flex flex-col gap-3 text-[13px]">
          <p>
            {t("This provider is connected in the master's configuration: its address depends on your account.")}{' '}
            {local
              ? t('A local machine needs no key.')
              : t("The key goes in a file that only the master's account can read, never in the configuration itself.")}
          </p>
          <pre className={cn('num overflow-x-auto rounded-md border bg-background px-4 py-3 text-[12.5px] leading-relaxed')}>
            {[
              `[models.${provider.id}]`,
              `provider = "${example.declare.provider}"`,
              ...(example.declare.nature !== 'llm' ? [`nature = "${example.declare.nature}"`] : []),
              ...(example.declare.vision ? ['vision = true'] : []),
              `base_url = "${provider.endpoint || t('the address given by your account')}"`,
              `model = "${example.id}"`,
              ...(local ? [] : [`api_key_file = "${t('/path/to/key-{id}', { id: provider.id })}"`]),
            ].join('\n')}
          </pre>
          <p className="text-muted-foreground">
            <Mixed text={t('Then add the alias to `models` in `[capabilities]`, and restart the master. An agent can use this model only if its parent grants it.')} />
            {example.declare.nature !== 'llm' && (
              <> <Mixed text={t('A service model is counted in calls: without {setting} under `[capabilities.max_calls]`, no call is allowed.')} slots={{ setting: <span className="num">{example.declare.nature} = …</span> }} /></>
            )}
          </p>
        </div>
      )}
    </section>
  )
}
