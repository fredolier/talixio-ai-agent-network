import { ShieldCheck } from 'lucide-react'
import { Empty, PageHeader, RuleLine, StatusMark } from '@/components/bits'
import { Badge } from '@/components/ui/badge'
import { flat, plural, sameRule } from '@/lib/format'
import type { Cluster } from '@/lib/store'
import type { Agent, ExecRule } from '@/lib/types'

/** Ce qui rend une commande déléguée sûre, dans l'ordre où c'est vérifié. */
const GUARDS = [
  ['Rien par défaut', 'Un agent sans règle ne peut lancer aucune commande : l’outil ne lui est même pas proposé.'],
  ['Jamais plus que le parent', 'Un parent ne délègue qu’une règle qu’il détient lui-même : même programme, même dossier, mêmes arguments. Si les siens sont libres, il peut les restreindre pour l’enfant, jamais l’inverse.'],
  ['Programme exact, sans interpréteur', 'Le programme est désigné par son chemin absolu et lancé directement, sans passer par un shell : pas de « ; », de « | » ni de « $( ) ».'],
  ['Arguments à leur place', 'Chaque argument est comparé à la règle, position par position. Un argument libre ne peut jamais commencer par « - » : il ne devient pas une option.'],
  ['Chemins revérifiés', 'Un argument {read} ou {write} doit être un chemin que l’agent a le droit de lire ou d’écrire, liens résolus ; les dossiers des clés sont toujours interdits.'],
  ['Vérifié à chaque appel', 'La règle est relue à chaque commande, pas une fois pour toutes. La commande tourne sans les variables secrètes du maître, 120 s au plus, et tout ce qu’elle a lancé est tué avec elle.'],
] as const

export function CommandsView({ cluster }: { cluster: Cluster }) {
  const master = cluster.tree
  const others = flat(master).filter((a) => a.id !== master.id)
  const rules = master.capabilities.exec ?? []
  const free = rules.filter((r) => r.args == null).length

  return (
    <div className="flex flex-col gap-6">
      <PageHeader
        eyebrow="Maîtrise · Commandes"
        title="Ce que la grappe a le droit d'exécuter"
        lead={
          rules.length
            ? `Le maître détient ${plural(rules.length, 'commande')}, écrites dans son fichier de configuration. Lui seul décide lesquelles il confie, et à qui.`
            : 'Le maître ne détient aucune commande : aucun agent de la grappe ne peut en exécuter.'
        }
      >
        {free > 0 && <Badge variant="destructive">{plural(free, 'règle')} à arguments libres</Badge>}
      </PageHeader>

      <div className="grid items-start gap-6 xl:grid-cols-[minmax(0,1fr)_340px]">
        <div className="flex flex-col gap-3">
          {rules.length === 0 && (
            <Empty title="Aucune commande autorisée">
              Ajoutez des règles dans la section <code>[capabilities]</code> du fichier de configuration du maître, puis relancez-le.
            </Empty>
          )}
          {rules.map((rule, index) => (
            <RuleCard key={index} rule={rule} holders={others.map((agent) => ({ agent, rules: (agent.capabilities.exec ?? []).filter((r) => r.program === rule.program && (r.cwd ?? null) === (rule.cwd ?? null)) })).filter((h) => h.rules.length > 0)} />
          ))}
        </div>

        <aside className="rounded-lg border bg-card p-5">
          <p className="eyebrow flex items-center gap-1.5"><ShieldCheck className="size-3.5" /> Comment un enfant est tenu</p>
          <ol className="mt-4 flex flex-col gap-4">
            {GUARDS.map(([title, body], index) => (
              <li key={title} className="grid grid-cols-[1.5rem_minmax(0,1fr)] gap-x-2">
                <span className="num text-xs text-muted-foreground">{String(index + 1).padStart(2, '0')}</span>
                <div>
                  <p className="font-medium">{title}</p>
                  <p className="mt-0.5 text-[13px] text-muted-foreground">{body}</p>
                </div>
              </li>
            ))}
          </ol>
          <p className="mt-5 border-t pt-4 text-[13px] text-muted-foreground">
            Limite connue : une commande autorisée s'exécute avec les droits du compte de l'agent. N'autorisez que des commandes dont vous acceptez tous les effets, et lancez le maître sous un compte dédié.
          </p>
        </aside>
      </div>
    </div>
  )
}

function RuleCard({ rule, holders }: { rule: ExecRule; holders: { agent: Agent; rules: ExecRule[] }[] }) {
  return (
    <section className="rounded-lg border bg-card">
      <div className="flex flex-wrap items-center justify-between gap-3 px-4 py-3">
        <RuleLine rule={rule} className="text-[13px]" />
        <span className="text-xs text-muted-foreground">
          {rule.cwd ? <>dans <code>{rule.cwd}</code></> : 'dans le dossier temporaire'}
        </span>
      </div>
      <div className="border-t bg-muted/40 px-4 py-2.5">
        {holders.length === 0 ? (
          <p className="text-[13px] text-muted-foreground">Gardée par le maître : confiée à aucun agent.</p>
        ) : (
          <ul className="flex flex-col gap-2">
            {holders.map(({ agent, rules }) => (
              <li key={agent.id} className="flex flex-wrap items-center gap-x-3 gap-y-1">
                <span className="eyebrow">confiée à</span>
                <span className="font-medium">{agent.name}</span>
                <StatusMark status={agent.status} />
                <span className="flex flex-wrap gap-x-3 gap-y-1">
                  {rules.map((own, index) =>
                    sameRule(own, rule) ? (
                      <span key={index} className="text-xs text-muted-foreground">telle quelle</span>
                    ) : (
                      <span key={index} className="inline-flex flex-wrap items-center gap-1.5 text-xs text-muted-foreground">
                        restreinte à <RuleLine rule={own} className="text-foreground" />
                      </span>
                    ),
                  )}
                </span>
              </li>
            ))}
          </ul>
        )}
      </div>
    </section>
  )
}
