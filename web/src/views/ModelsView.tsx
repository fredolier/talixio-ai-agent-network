import { KeyRound } from 'lucide-react'
import { Empty, PageHeader } from '@/components/bits'
import { Badge } from '@/components/ui/badge'
import { Skeleton } from '@/components/ui/skeleton'
import { pick } from '@/lib/utils'
import { count, flat, plural } from '@/lib/format'
import type { Cluster } from '@/lib/store'
import type { ModelInfo } from '@/lib/types'

const PROVIDER: Record<string, string> = { anthropic: 'Anthropic', openai: 'Compatible OpenAI' }

export function ModelsView({ cluster }: { cluster: Cluster }) {
  const agents = flat(cluster.tree)
  const data = cluster.models
  const groups = new Map<string, ModelInfo[]>()
  data?.models.forEach((m) => {
    const key = `${m.provider}\u0000${m.host}`
    groups.set(key, [...(groups.get(key) ?? []), m])
  })

  return (
    <div className="flex flex-col gap-6">
      <PageHeader
        eyebrow="Maîtrise · Modèles"
        title="Fournisseurs et modèles"
        lead="Les clés ne quittent jamais le maître : chaque appel d'un agent remonte jusqu'à lui, qui vérifie le droit et le budget avant d'interroger le fournisseur."
      />
      {!data ? (
        <Skeleton className="h-40 rounded-lg" />
      ) : data.models.length === 0 ? (
        <Empty title="Aucun modèle configuré" />
      ) : (
        [...groups].map(([key, models]) => {
          const [provider, host] = key.split('\u0000')
          return (
            <section key={key} className="flex flex-col gap-3">
              <div className="flex flex-wrap items-baseline gap-x-3">
                <h2 className="text-base font-semibold">{pick(PROVIDER, provider) ?? provider}</h2>
                <span className="num text-xs text-muted-foreground">{host}</span>
                <span className="text-xs text-muted-foreground">· {plural(models.length, 'modèle')}</span>
              </div>
              <div className="grid gap-3 md:grid-cols-2 2xl:grid-cols-3">
                {models.map((model) => {
                  const running = agents.filter((a) => a.model === model.alias)
                  const granted = agents.filter((a) => a.depth > 0 && a.capabilities.models?.includes(model.alias))
                  const tokens = running.reduce((sum, a) => sum + (a.depth === 0 ? 0 : a.tokens_used), 0)
                  return (
                    <article key={model.alias} className="flex flex-col gap-3 rounded-lg border bg-card p-4">
                      <div className="flex flex-wrap items-center gap-2">
                        <h3 className="font-mono text-[15px] font-medium">{model.alias}</h3>
                        {model.alias === data.default && <Badge>modèle du maître</Badge>}
                        <span className="ml-auto inline-flex items-center gap-1 text-xs text-muted-foreground" title="La clé n'est jamais affichée ni transmise">
                          <KeyRound className="size-3.5" />
                          {model.has_key ? 'clé en place' : 'sans clé'}
                        </span>
                      </div>
                      <dl className="grid grid-cols-[max-content_minmax(0,1fr)] gap-x-4 gap-y-1.5 text-[13px]">
                        <dt className="eyebrow pt-0.5">Modèle</dt>
                        <dd className="font-mono [overflow-wrap:anywhere]">{model.model}</dd>
                        <dt className="eyebrow pt-0.5">Effort</dt>
                        <dd>{model.effort ?? <span className="text-muted-foreground">par défaut</span>}</dd>
                        <dt className="eyebrow pt-0.5">Réponse</dt>
                        <dd>{model.max_tokens ? `${count(model.max_tokens)} tokens au plus` : <span className="text-muted-foreground">par défaut</span>}</dd>
                      </dl>
                      <div className="border-t pt-3 text-[13px]">
                        <p>
                          <span className="num font-medium">{running.length}</span>{' '}
                          <span className="text-muted-foreground">agent{running.length > 1 ? 's' : ''} l'utilise{running.length > 1 ? 'nt' : ''}</span>
                          {tokens > 0 && <span className="text-muted-foreground"> · <span className="num">{count(tokens)}</span> tokens chez les enfants</span>}
                        </p>
                        <p className="mt-1 text-muted-foreground">
                          {granted.length ? <>Accordé à {granted.map((a) => a.name).join(', ')}.</> : 'Accordé à aucun enfant.'}
                        </p>
                      </div>
                    </article>
                  )
                })}
              </div>
            </section>
          )
        })
      )}
      <p className="text-[13px] text-muted-foreground">
        Pour l'instant, tous les modèles sont des modèles de texte. Le catalogue par nature (texte, image, voix) est inscrit dans la spécification et reste à construire.
      </p>
    </div>
  )
}
