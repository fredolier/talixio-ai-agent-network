import type { ReactNode } from 'react'
import { cn } from '@/lib/utils'
import { STATUS } from '@/lib/format'
import type { ExecRule, Status } from '@/lib/types'

const DOT: Record<string, string> = {
  working: 'bg-st-working live-dot',
  starting: 'bg-st-working',
  idle: 'bg-st-idle',
  suspended: 'bg-st-wait',
  disconnected: 'bg-st-off',
  stopped: 'bg-transparent ring-1 ring-inset ring-st-off',
}

/** L'état d'un agent : un point et un mot, jamais une pastille de couleur pleine. */
export function StatusMark({ status, className }: { status: Status | string; className?: string }) {
  return (
    <span className={cn('inline-flex items-center gap-1.5 text-xs whitespace-nowrap text-muted-foreground', className)}>
      <span className={cn('size-2 rounded-full', DOT[status] ?? 'bg-st-idle')} aria-hidden />
      {STATUS[status as Status] ?? status}
    </span>
  )
}

export function PageHeader({ eyebrow, title, lead, children }: { eyebrow: string; title: string; lead?: ReactNode; children?: ReactNode }) {
  return (
    <header className="flex flex-wrap items-end justify-between gap-x-6 gap-y-3 border-b pb-5">
      <div className="min-w-0">
        <p className="eyebrow">{eyebrow}</p>
        <h1 className="mt-1 text-[26px] leading-tight font-semibold tracking-tight">{title}</h1>
        {lead && <p className="mt-1.5 max-w-2xl text-muted-foreground">{lead}</p>}
      </div>
      {children}
    </header>
  )
}

export function Empty({ title, children }: { title: string; children?: ReactNode }) {
  return (
    <div className="rounded-lg border border-dashed px-6 py-10 text-center">
      <p className="font-medium">{title}</p>
      {children && <p className="mx-auto mt-1 max-w-md text-muted-foreground">{children}</p>}
    </div>
  )
}

export function Field({ label, children, mono }: { label: string; children: ReactNode; mono?: boolean }) {
  return (
    <>
      <dt className="eyebrow pt-0.5">{label}</dt>
      <dd className={cn('min-w-0 [overflow-wrap:anywhere]', mono && 'font-mono text-[12.5px]')}>{children}</dd>
    </>
  )
}

const TOKEN: Record<string, { label: string; className: string }> = {
  '*': { label: 'un argument libre, jamais une option', className: 'border-st-wait/50 text-st-wait' },
  '**': { label: 'une suite d’arguments libres, aucune option', className: 'border-st-wait/50 text-st-wait' },
  '{read}': { label: 'un chemin que l’agent a le droit de lire', className: 'border-st-working/50 text-st-working' },
  '{write}': { label: 'un chemin que l’agent a le droit d’écrire', className: 'border-st-working/50 text-st-working' },
}

/** Une règle de commande, telle qu'elle est vérifiée : programme, puis chaque argument à sa place. */
export function RuleLine({ rule, className }: { rule: ExecRule; className?: string }) {
  return (
    <span className={cn('inline-flex flex-wrap items-center gap-1.5 font-mono text-[12.5px]', className)}>
      <span className="font-medium">{rule.program}</span>
      {rule.args == null ? (
        <span className="rounded border border-st-off/50 px-1.5 py-px text-st-off" title="Aucune contrainte sur les arguments : tous les pouvoirs du programme">
          arguments libres
        </span>
      ) : rule.args.length === 0 ? (
        <span className="text-muted-foreground">(sans argument)</span>
      ) : (
        rule.args.map((arg, index) =>
          TOKEN[arg] ? (
            <span key={index} title={TOKEN[arg].label} className={cn('rounded border px-1.5 py-px', TOKEN[arg].className)}>
              {arg}
            </span>
          ) : (
            <span key={index}>{arg}</span>
          ),
        )
      )}
    </span>
  )
}

/** `code` entre accents graves, sans jamais passer par du HTML. */
export function Rich({ text }: { text: string }) {
  return <>{text.split('`').map((part, index) => (index % 2 ? <code key={index}>{part}</code> : part))}</>
}
