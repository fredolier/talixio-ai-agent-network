import { ShieldCheck } from 'lucide-react'
import { Empty, PageHeader, Rich, RuleLine, StatusMark } from '@/components/bits'
import { Badge } from '@/components/ui/badge'
import { flat, sameRule } from '@/lib/format'
import { t, tn, tx } from '@/lib/i18n'
import type { Cluster } from '@/lib/store'
import type { Agent, ExecRule } from '@/lib/types'

/** What makes a delegated command safe, in the order it is checked. */
const GUARDS = [
  [tx('Nothing by default'), tx('An agent without a rule cannot run any command: the tool is not even offered to it.')],
  [tx('Never more than the parent'), tx('A parent only delegates a rule it holds itself: same program, same folder, same arguments. If its own are free, it can restrict them for the child, never the reverse.')],
  [tx('Exact program, no interpreter'), tx('The program is named by its absolute path and started directly, without going through a shell: no “;”, “|” or “$( )”.')],
  [tx('Arguments in their place'), tx('Each argument is compared with the rule, position by position. A free argument can never start with “-”: it does not become an option.')],
  [tx('Paths checked again'), tx('A {read} or {write} argument must be a path the agent is allowed to read or write, links resolved; the key folders are always forbidden.')],
  [tx('Checked on every call'), tx('The rule is read again for every command, not once and for all. The command runs without the secret variables of the master, 120 s at most, and everything it started is killed with it.')],
] as const

export function CommandsView({ cluster }: { cluster: Cluster }) {
  const master = cluster.tree
  const others = flat(master).filter((a) => a.id !== master.id)
  const rules = master.capabilities.exec ?? []
  const free = rules.filter((r) => r.args == null).length

  return (
    <div className="flex flex-col gap-6">
      <PageHeader
        eyebrow={t('Control · Commands')}
        title={t('What the cluster is allowed to run')}
        lead={
          rules.length
            ? t('The master holds {commands}, written in its configuration file. It alone decides which ones it hands over, and to whom.', {
                commands: tn(rules.length, '{n} command', '{n} commands'),
              })
            : t('The master holds no command: no agent in the cluster can run any.')
        }
      >
        {free > 0 && <Badge variant="destructive">{tn(free, '{n} rule with free arguments', '{n} rules with free arguments')}</Badge>}
      </PageHeader>

      <div className="grid items-start gap-6 xl:grid-cols-[minmax(0,1fr)_340px]">
        <div className="flex flex-col gap-3">
          {rules.length === 0 && (
            <Empty title={t('No command allowed')}>
              <Rich text={t('Add rules in the `[capabilities]` section of the configuration file of the master, then restart it.')} />
            </Empty>
          )}
          {rules.map((rule, index) => (
            <RuleCard key={index} rule={rule} holders={others.map((agent) => ({ agent, rules: (agent.capabilities.exec ?? []).filter((r) => r.program === rule.program && (r.cwd ?? null) === (rule.cwd ?? null)) })).filter((h) => h.rules.length > 0)} />
          ))}
        </div>

        <aside className="rounded-lg border bg-card p-5">
          <p className="eyebrow flex items-center gap-1.5"><ShieldCheck className="size-3.5" /> {t('How a child is kept in check')}</p>
          <ol className="mt-4 flex flex-col gap-4">
            {GUARDS.map(([title, body], index) => (
              <li key={title} className="grid grid-cols-[1.5rem_minmax(0,1fr)] gap-x-2">
                <span className="num text-xs text-muted-foreground">{String(index + 1).padStart(2, '0')}</span>
                <div>
                  <p className="font-medium">{t(title)}</p>
                  <p className="mt-0.5 text-[13px] text-muted-foreground">{t(body)}</p>
                </div>
              </li>
            ))}
          </ol>
          <p className="mt-5 border-t pt-4 text-[13px] text-muted-foreground">
            {t('Known limit: an allowed command runs with the permissions of the account of the agent. Only allow commands whose every effect you accept, and run the master under a dedicated account.')}
          </p>
        </aside>
      </div>
    </div>
  )
}

function RuleCard({ rule, holders }: { rule: ExecRule; holders: { agent: Agent; rules: ExecRule[] }[] }) {
  // the folder is data: it is placed in the sentence, never read as text to format
  const [before, after = ''] = t('in {cwd}').split('{cwd}')
  return (
    <section className="rounded-lg border bg-card">
      <div className="flex flex-wrap items-center justify-between gap-3 px-4 py-3">
        <RuleLine rule={rule} className="text-[13px]" />
        <span className="text-xs text-muted-foreground">
          {rule.cwd ? <>{before}<code>{rule.cwd}</code>{after}</> : t('in the temporary folder')}
        </span>
      </div>
      <div className="border-t bg-muted/40 px-4 py-2.5">
        {holders.length === 0 ? (
          <p className="text-[13px] text-muted-foreground">{t('Kept by the master: handed to no agent.')}</p>
        ) : (
          <ul className="flex flex-col gap-2">
            {holders.map(({ agent, rules }) => (
              <li key={agent.id} className="flex flex-wrap items-center gap-x-3 gap-y-1">
                <span className="eyebrow">{t('handed to')}</span>
                <span className="font-medium">{agent.name}</span>
                <StatusMark status={agent.status} />
                <span className="flex flex-wrap gap-x-3 gap-y-1">
                  {rules.map((own, index) =>
                    sameRule(own, rule) ? (
                      <span key={index} className="text-xs text-muted-foreground">{t('as is')}</span>
                    ) : (
                      <span key={index} className="inline-flex flex-wrap items-center gap-1.5 text-xs text-muted-foreground">
                        {t('restricted to')} <RuleLine rule={own} className="text-foreground" />
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
