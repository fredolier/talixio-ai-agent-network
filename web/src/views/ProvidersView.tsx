import { useState } from 'react'
import { Connection, Origin, Trust, TRUST_LEVELS } from '@/components/catalog'
import { PageHeader } from '@/components/bits'
import { Checkbox } from '@/components/ui/checkbox'
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
        lead="Tous les fournisseurs que le maître connaît, d'où qu'ils soient. Un fournisseur est connecté quand un de ses modèles est déclaré chez le maître avec sa clé. La clé reste chez le maître : aucun agent ne la reçoit."
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

      {selected ? <ProviderDetail provider={selected} data={data} /> : <p className="text-[13px] text-muted-foreground">Choisissez un fournisseur pour voir comment s'y connecter.</p>}

      <p className="flex flex-wrap items-center gap-x-4 gap-y-2 border-t pt-4 text-[13px] text-muted-foreground">
        <span className="eyebrow">Confiance</span>
        {TRUST_LEVELS.map(({ level, label }) => (
          <span key={level} className="inline-flex items-center gap-1.5"><Trust level={level} /> {label}</span>
        ))}
      </p>
    </div>
  )
}

function ProviderDetail({ provider, data }: { provider: CatalogProvider; data: ModelsData }) {
  const models = data.catalog.filter((m) => m.provider === provider.id)
  const active = models.filter((m) => m.aliases.length > 0)
  // le catalogue ne connaît pas toujours le nom exact attendu par l'API : ne jamais en inventer un
  const example = models.find((m) => m.callable && !m.deprecated && /^[a-z0-9][a-z0-9._:/-]*$/.test(m.id))
  const local = provider.trust === 'L'
  return (
    <section className="rounded-lg border bg-card p-5">
      <div className="flex flex-wrap items-baseline gap-x-4 gap-y-1">
        <h2 className="text-base font-semibold">{provider.name}</h2>
        <span className="text-[13px] text-muted-foreground">{provider.kind}</span>
        <a href={`#modeles?f=${encodeURIComponent(provider.id)}`} className="ml-auto text-[13px] underline underline-offset-4">
          Voir ses {plural(models.length, 'modèle')}
        </a>
      </div>
      {provider.connected ? (
        <p className="mt-3 text-[13px]">
          Connecté. {active.length > 0 ? 'Activés chez le maître : ' : 'Déclaré chez le maître.'}
          {active.map((m, index) => (
            <span key={m.id}>
              {index > 0 && ', '}
              {m.name} <span className="text-muted-foreground">(alias <span className="num">{m.aliases.join(', ')}</span>)</span>
            </span>
          ))}
        </p>
      ) : !provider.api ? (
        <p className="mt-3 text-[13px] text-muted-foreground">
          Ce fournisseur ne sert que de la voix ou de l'image : le maître ne sait pas encore appeler ces modèles pour un agent. Il est listé pour comparer.
        </p>
      ) : (
        <div className="mt-3 flex flex-col gap-3 text-[13px]">
          <p>
            La connexion se fait aujourd'hui dans la configuration du maître, pas depuis cette page.{' '}
            {local
              ? "Une machine locale ne demande pas de clé."
              : "La clé va dans un fichier que seul le compte du maître peut lire, jamais dans la configuration elle-même."}
          </p>
          <pre className={cn('num overflow-x-auto rounded-md border bg-background px-4 py-3 text-[12.5px] leading-relaxed')}>
            {[
              `[models.${provider.id}]`,
              `provider = "${provider.api}"`,
              `base_url = "${provider.endpoint || "l'adresse donnée par votre compte"}"`,
              `model = "${example?.id ?? 'le nom exact du modèle chez ce fournisseur'}"`,
              ...(local ? [] : [`api_key_file = "/chemin/vers/cle-${provider.id}"`]),
            ].join('\n')}
          </pre>
          <p className="text-muted-foreground">
            Ajoutez ensuite l'alias à <span className="num">models</span> dans <span className="num">[capabilities]</span>, puis redémarrez le maître. Un agent ne peut utiliser ce modèle que si son parent le lui accorde.
          </p>
        </div>
      )}
    </section>
  )
}
