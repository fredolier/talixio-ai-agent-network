import { Component, type ReactNode } from 'react'
import { cn, pick } from '@/lib/utils'
import { statusLabel } from '@/lib/format'
import { t, tx } from '@/lib/i18n'
import type { ExecRule, Status } from '@/lib/types'

const DOT: Record<string, string> = {
  working: 'bg-st-working live-dot',
  starting: 'bg-st-working',
  idle: 'bg-st-idle',
  suspended: 'bg-st-wait',
  disconnected: 'bg-st-off',
  stopped: 'bg-transparent ring-1 ring-inset ring-st-off',
}

/** The status of an agent: a dot and a word, never a solid colour pill. */
export function StatusMark({ status, className }: { status: Status | string; className?: string }) {
  return (
    <span className={cn('inline-flex items-center gap-1.5 text-xs whitespace-nowrap text-muted-foreground', className)}>
      <span className={cn('size-2 rounded-full', pick(DOT, status) ?? 'bg-st-idle')} aria-hidden />
      {statusLabel(status)}
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
  '*': { label: tx('one free argument, never an option'), className: 'border-st-wait/50 text-st-wait' },
  '**': { label: tx('any number of free arguments, no option'), className: 'border-st-wait/50 text-st-wait' },
  '{read}': { label: tx('a path the agent is allowed to read'), className: 'border-st-working/50 text-st-working' },
  '{write}': { label: tx('a path the agent is allowed to write'), className: 'border-st-working/50 text-st-working' },
}

/** A command rule, as it is checked: the program, then each argument in its place. */
export function RuleLine({ rule, className }: { rule: ExecRule; className?: string }) {
  return (
    <span className={cn('inline-flex flex-wrap items-center gap-1.5 font-mono text-[12.5px]', className)}>
      <span className="font-medium">{rule.program}</span>
      {rule.args == null ? (
        <span className="rounded border border-st-off/50 px-1.5 py-px text-st-off" title={t('No constraint on the arguments: every power of the program')}>
          {t('free arguments')}
        </span>
      ) : rule.args.length === 0 ? (
        <span className="text-muted-foreground">{t('(no argument)')}</span>
      ) : (
        rule.args.map((arg, index) => {
          const token = pick(TOKEN, arg)
          return token ? (
            <span key={index} title={t(token.label)} className={cn('rounded border px-1.5 py-px', token.className)}>
              {arg}
            </span>
          ) : (
            <span key={index}>{String(arg)}</span>
          )
        })
      )}
    </span>
  )
}

/** `code` between backticks, never going through HTML. */
export function Rich({ text }: { text: string }) {
  return <>{text.split('`').map((part, index) => (index % 2 ? <code key={index}>{part}</code> : part))}</>
}

/** A screen that fails (unexpected data) does not blank the whole page: we say so, and the rest holds. */
export class Guard extends Component<{ children: ReactNode }, { failed: boolean }> {
  state = { failed: false }
  static getDerivedStateFromError() {
    return { failed: true }
  }
  render() {
    if (!this.state.failed) return this.props.children
    return (
      <div role="alert" className="rounded-md border border-st-off/50 px-4 py-3">
        <p className="font-medium">{t('This screen could not be displayed.')}</p>
        <p className="mt-1 text-muted-foreground">{t('Some received data did not have the expected shape. The other screens remain usable.')}</p>
        <button type="button" className="mt-2 underline underline-offset-4" onClick={() => this.setState({ failed: false })}>
          {t('Try again')}
        </button>
      </div>
    )
  }
}
