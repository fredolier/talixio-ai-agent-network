import { useState, type FormEvent } from 'react'
import { Connection, Origin, Trust, TRUST_LEVELS } from '@/components/catalog'
import { PageHeader } from '@/components/bits'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Skeleton } from '@/components/ui/skeleton'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { plural } from '@/lib/format'
import type { Cluster } from '@/lib/store'
import type { CatalogProvider, ModelsData } from '@/lib/types'
import { cn } from '@/lib/utils'

export function ProvidersView({ cluster }: { cluster: Cluster }) {
  const data = cluster.models
  const [europe, setEurope] = useState(false)
  const [connected, setConnected] = useState(false)
  const [open, setOpen] = useState<string | null>(null)
  if (!data) return <Skeleton className="h-64 rounded-lg" />

  // connectés d'abord, puis européens, puis par nom : ce qui sert est en haut
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
        eyebrow="Maîtrise · Fournisseurs"
        title="Fournisseurs"
        lead="Tous les fournisseurs que le maître connaît, d'où qu'ils soient. Choisissez-en un pour vous y connecter d'ici, avec votre clé. La clé reste chez le maître : aucun agent ne la reçoit, et cette page ne la relit jamais."
      />
      <div className="flex flex-wrap items-center gap-x-6 gap-y-3">
        <p className="text-[13px] text-muted-foreground">
          <span className="num font-medium text-foreground">{on}</span> connecté{on > 1 ? 's' : ''} sur <span className="num">{total}</span> ·{' '}
          <span className="num">{european}</span> européen{european > 1 ? 's' : ''}
        </p>
        <div className="ml-auto flex flex-wrap gap-x-5 gap-y-2">
          <Label className="font-normal">
            <Checkbox checked={europe} onCheckedChange={(checked) => setEurope(checked === true)} />
            Europe et local seulement
          </Label>
          <Label className="font-normal">
            <Checkbox checked={connected} onCheckedChange={(checked) => setConnected(checked === true)} />
            Connectés seulement
          </Label>
        </div>
      </div>

      <div className="rounded-lg border bg-card">
        <Table>
          <TableHeader>
            <TableRow className="hover:bg-transparent">
              <TableHead className="eyebrow px-4">Fournisseur</TableHead>
              <TableHead className="eyebrow">Pays</TableHead>
              <TableHead className="eyebrow">Origine</TableHead>
              <TableHead className="eyebrow">Confiance</TableHead>
              <TableHead className="eyebrow">Facturation</TableHead>
              <TableHead className="eyebrow text-right">Modèles</TableHead>
              <TableHead className="eyebrow px-4">État</TableHead>
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
                    {!provider.listed && <span className="ml-2 text-xs text-muted-foreground">hors catalogue</span>}
                  </TableCell>
                  <TableCell>{provider.country || <span className="text-muted-foreground">non renseigné</span>}</TableCell>
                  <TableCell><Origin provider={provider} /></TableCell>
                  <TableCell><Trust level={provider.trust} withLabel /></TableCell>
                  <TableCell className="text-muted-foreground">{provider.billing === 'usage' ? "à l'usage" : provider.billing || '—'}</TableCell>
                  <TableCell className="text-right">
                    <span className="num">{known.length}</span>
                    {active > 0 && <span className="text-muted-foreground"> · <span className="num text-foreground">{active}</span> activé{active > 1 ? 's' : ''}</span>}
                  </TableCell>
                  <TableCell className="px-4"><Connection on={provider.connected} /></TableCell>
                </TableRow>
              )
            })}
            {rows.length === 0 && (
              <TableRow className="hover:bg-transparent">
                <TableCell colSpan={7} className="px-4 py-8 text-center text-muted-foreground">Aucun fournisseur ne correspond à ces filtres.</TableCell>
              </TableRow>
            )}
          </TableBody>
        </Table>
      </div>

      {cluster.notice && <p role="status" className="rounded-md border bg-card px-4 py-3 text-[13px]">{cluster.notice}</p>}

      {selected ? <ProviderDetail key={selected.id} provider={selected} data={data} cluster={cluster} /> : <p className="text-[13px] text-muted-foreground">Choisissez un fournisseur pour voir comment s'y connecter.</p>}

      <p className="flex flex-wrap items-center gap-x-4 gap-y-2 border-t pt-4 text-[13px] text-muted-foreground">
        <span className="eyebrow">Confiance</span>
        {TRUST_LEVELS.map(({ level, label }) => (
          <span key={level} className="inline-flex items-center gap-1.5"><Trust level={level} /> {label}</span>
        ))}
      </p>
    </div>
  )
}

function ProviderDetail({ provider, data, cluster }: { provider: CatalogProvider; data: ModelsData; cluster: Cluster }) {
  const models = data.catalog.filter((m) => m.provider === provider.id)
  const active = models.filter((m) => m.aliases.length > 0)
  // le catalogue ne connaît pas toujours le nom exact attendu par l'API : ne jamais en inventer un
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
      setMessage({ ok: false, text: error instanceof Error ? error.message : 'Demande non aboutie.' })
    } finally {
      setBusy(false)
    }
  }
  const connect = (event: FormEvent) => {
    event.preventDefault()
    run(async () => {
      const done = await cluster.change<{ verified: boolean }>(`${path}/connect`, { key })
      setKey('')
      return done.verified ? 'Connecté : le fournisseur a accepté la clé.' : "Connecté. Le fournisseur n'a pas permis de vérifier la clé : elle le sera au premier appel."
    })
  }
  const disconnect = () => {
    if (!confirm(`Déconnecter ${provider.name} ? Sa clé est effacée du maître et ses modèles activés d'ici sont retirés.`)) return
    run(async () => {
      await cluster.change(`${path}/disconnect`, {})
      return 'Déconnecté : la clé est effacée.'
    })
  }
  const activate = (event: FormEvent) => {
    event.preventDefault()
    run(async () => {
      const done = await cluster.change<{ alias: string }>('/api/models/activate', { provider: provider.id, model: other.trim() })
      setOther('')
      return `Modèle activé sous l'alias « ${done.alias} ».`
    })
  }
  const feedback = message && <p role={message.ok ? 'status' : 'alert'} className={cn('text-[13px]', !message.ok && 'text-destructive')}>{message.text}</p>
  return (
    <section className="rounded-lg border bg-card p-5">
      <div className="flex flex-wrap items-baseline gap-x-4 gap-y-1">
        <h2 className="text-base font-semibold">{provider.name}</h2>
        <span className="text-[13px] text-muted-foreground">{provider.kind}</span>
        <a href={`#modeles?f=${encodeURIComponent(provider.id)}`} className="ml-auto text-[13px] underline underline-offset-4">
          Voir ses {plural(models.length, 'modèle')}
        </a>
      </div>
      {provider.linked ? (
        <div className="mt-3 flex flex-col gap-3 text-[13px]">
          <p>
            Connecté depuis cette page. {active.length > 0 ? 'Activés chez le maître : ' : "Aucun modèle activé pour l'instant : "}
            {active.map((m, index) => (
              <span key={m.id}>
                {index > 0 && ', '}
                {m.name} <span className="text-muted-foreground">(alias <span className="num">{m.aliases.join(', ')}</span>)</span>
              </span>
            ))}
            {active.length === 0 && <a href={`#modeles?f=${encodeURIComponent(provider.id)}`} className="underline underline-offset-4">activez-en un dans la page Modèles</a>}
            .
          </p>
          <form onSubmit={activate} className="flex flex-wrap items-end gap-3">
            <div className="flex min-w-64 flex-1 flex-col gap-1.5">
              <Label htmlFor="other-model">Un modèle absent du catalogue</Label>
              <Input id="other-model" value={other} onChange={(e) => setOther(e.target.value)} placeholder="son nom exact chez le fournisseur" className="font-mono" autoComplete="off" spellCheck={false} />
            </div>
            <Button type="submit" variant="outline" disabled={busy || !other.trim()}>Activer</Button>
            <Button type="button" variant="outline" disabled={busy} onClick={disconnect} className="ml-auto">Déconnecter</Button>
          </form>
          {feedback}
        </div>
      ) : provider.connect && provider.connect !== 'config' && !provider.connected ? (
        <div className="mt-3 flex flex-col gap-3 text-[13px]">
          {provider.connect === 'local' ? (
            <>
              <p>Une machine locale ne demande pas de clé. Le maître vérifie qu'un serveur de modèles répond à <span className="num">{provider.endpoint}</span>.</p>
              <div><Button disabled={busy} onClick={() => run(async () => { await cluster.change(`${path}/connect`, { key: '' }); return 'Connecté.' })}>Connecter</Button></div>
            </>
          ) : (
            <>
              {provider.connect === 'oauth' && (
                <div className="flex flex-wrap items-center gap-3">
                  <Button disabled={busy} onClick={() => run(async () => { await cluster.authorize(provider.id); return 'Ouverture de la page du fournisseur…' })}>
                    Se connecter avec {provider.name}
                  </Button>
                  <span className="text-muted-foreground">Vous autorisez le maître chez le fournisseur, sans copier de clé. Sinon, collez une clé :</span>
                </div>
              )}
              {provider.keyless && (
                <div className="flex flex-wrap items-center gap-3">
                  <Button disabled={busy} onClick={() => run(async () => { await cluster.change(`${path}/connect`, { key: '' }); return 'Connecté sans clé.' })}>
                    Connecter sans clé
                  </Button>
                  <span className="text-muted-foreground">{provider.name} répond aussi sans compte, avec des limites basses : de quoi essayer. Pour un usage suivi, collez une clé :</span>
                </div>
              )}
              <form onSubmit={connect} className="flex flex-wrap items-end gap-3">
                <div className="flex min-w-64 flex-1 flex-col gap-1.5">
                  <Label htmlFor="provider-key">Clé d'API {provider.name}</Label>
                  <Input id="provider-key" type="password" value={key} onChange={(e) => setKey(e.target.value)} className="font-mono" autoComplete="off" spellCheck={false} />
                </div>
                <Button type="submit" variant={provider.connect === 'oauth' || provider.keyless ? 'outline' : 'default'} disabled={busy || key.trim().length < 8}>Connecter</Button>
                {provider.keys_url && (
                  <a href={provider.keys_url} target="_blank" rel="noopener noreferrer" className="pb-2 underline underline-offset-4">Obtenir une clé</a>
                )}
              </form>
              <p className="text-muted-foreground">
                Le maître essaie la clé chez le fournisseur, puis la range dans un fichier que seul son compte peut lire. Elle n'est plus jamais affichée, ni donnée à un agent.
              </p>
            </>
          )}
          {feedback}
        </div>
      ) : provider.connected ? (
        <p className="mt-3 text-[13px]">
          Connecté par la configuration du maître. {active.length > 0 ? 'Activés : ' : ''}
          {active.map((m, index) => (
            <span key={m.id}>
              {index > 0 && ', '}
              {m.name} <span className="text-muted-foreground">(alias <span className="num">{m.aliases.join(', ')}</span>)</span>
            </span>
          ))}
        </p>
      ) : !provider.apis?.length || !example?.declare ? (
        <p className="mt-3 text-[13px] text-muted-foreground">
          Le maître n'a pas encore de connecteur pour les modèles de ce fournisseur. Il est listé pour comparer.
        </p>
      ) : (
        <div className="mt-3 flex flex-col gap-3 text-[13px]">
          <p>
            Ce fournisseur se connecte dans la configuration du maître : son adresse dépend de votre compte.{' '}
            {local
              ? "Une machine locale ne demande pas de clé."
              : "La clé va dans un fichier que seul le compte du maître peut lire, jamais dans la configuration elle-même."}
          </p>
          <pre className={cn('num overflow-x-auto rounded-md border bg-background px-4 py-3 text-[12.5px] leading-relaxed')}>
            {[
              `[models.${provider.id}]`,
              `provider = "${example.declare.provider}"`,
              ...(example.declare.nature !== 'llm' ? [`nature = "${example.declare.nature}"`] : []),
              ...(example.declare.vision ? ['vision = true'] : []),
              `base_url = "${provider.endpoint || "l'adresse donnée par votre compte"}"`,
              `model = "${example.id}"`,
              ...(local ? [] : [`api_key_file = "/chemin/vers/cle-${provider.id}"`]),
            ].join('\n')}
          </pre>
          <p className="text-muted-foreground">
            Ajoutez ensuite l'alias à <span className="num">models</span> dans <span className="num">[capabilities]</span>, puis redémarrez le maître. Un agent ne peut utiliser ce modèle que si son parent le lui accorde.
            {example.declare.nature !== 'llm' && (
              <> Un modèle de service se compte en appels : sans <span className="num">{example.declare.nature} = …</span> sous <span className="num">[capabilities.max_calls]</span>, aucun appel n'est permis.</>
            )}
          </p>
        </div>
      )}
    </section>
  )
}
