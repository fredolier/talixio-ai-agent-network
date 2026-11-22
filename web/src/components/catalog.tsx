import { Fragment, type ReactNode } from 'react'
import { cn, pick } from '@/lib/utils'
import { currentLang, t, tx } from '@/lib/i18n'
import type { CatalogModel, CatalogProvider } from '@/lib/types'

/** A translated text with elements inside: `{name}` is replaced by `slots.name`, and text between backticks is set as a technical name. */
export function Mixed({ text, slots = {} }: { text: string; slots?: Record<string, ReactNode> }) {
  return (
    <>
      {text.split('`').map((part, index) =>
        index % 2 ? (
          <span key={index} className="num">{part}</span>
        ) : (
          <Fragment key={index}>
            {part.split(/(\{\w+\})/).map((piece, at) => (at % 2 && Object.hasOwn(slots, piece.slice(1, -1)) ? <Fragment key={at}>{slots[piece.slice(1, -1)]}</Fragment> : piece))}
          </Fragment>
        ),
      )}
    </>
  )
}

const TRUST: Record<string, { label: string; className: string }> = {
  '1': { label: tx('consumer'), className: 'border-st-off/60 text-st-off' },
  '2': { label: tx('free or public'), className: 'border-st-wait/70 text-foreground' },
  '3': { label: tx('professional'), className: 'border-border text-foreground' },
  '4': { label: tx('no retention'), className: 'border-foreground/40 text-foreground' },
  '5': { label: tx('sovereign (GDPR)'), className: 'border-foreground bg-foreground text-background' },
  L: { label: tx('local'), className: 'border-foreground bg-foreground text-background' },
}

/** The trust levels, for the legend; `label` is translated with `t(label)` when rendering. */
export const TRUST_LEVELS = Object.entries(TRUST).map(([level, { label }]) => ({ level, label }))

/** The trust level of a provider: a digit in a square, and what it means. */
export function Trust({ level, withLabel }: { level: string; withLabel?: boolean }) {
  const known = pick(TRUST, level)
  if (!known) return <span className="text-muted-foreground">{t('not rated')}</span>
  return (
    <span className="inline-flex items-center gap-2 whitespace-nowrap">
      <span className={cn('num grid size-5 place-items-center rounded-[5px] border text-[11px] font-medium', known.className)}>{level}</span>
      {withLabel && <span className="text-[13px]">{t(known.label)}</span>}
    </span>
  )
}

/** European, outside Europe, or on your own machine. */
export function Origin({ provider }: { provider: CatalogProvider }) {
  if (provider.trust === 'L') return <span>{t('Local')}</span>
  if (provider.europe === null) return <span className="text-muted-foreground">{t('not specified')}</span>
  return provider.europe ? <span className="font-medium">{t('Europe')}</span> : <span className="text-muted-foreground">{t('Outside Europe')}</span>
}

/** The relative quality of a model, from 1 to 10: ten notches, not a gradient. */
export function Quality({ rank }: { rank: number | null }) {
  if (rank === null) return <span className="text-muted-foreground">{t('not ranked')}</span>
  return (
    <span className="inline-flex items-center gap-2" title={t('Quality {rank} out of 10', { rank })}>
      <span className="flex gap-px" aria-hidden>
        {Array.from({ length: 10 }, (_, i) => (
          <span key={i} className={cn('h-3 w-[5px] rounded-[1px]', i < rank ? 'bg-foreground' : 'bg-border')} />
        ))}
      </span>
      <span className="num w-4 text-right text-[13px]">{rank}</span>
    </span>
  )
}

const NATURE: Record<string, string> = {
  LLM: tx('Text'),
  Vision: tx('Vision'),
  Voice: tx('Voice'),
  Image: tx('Image'),
  Embedding: tx('Embeddings'),
  Forecast: tx('Forecast'),
  Decision: tx('Decision'),
}
/** The word for a kind of model, in the current language; an unknown kind is shown as received. */
export const natureLabel = (nature: string) => {
  const known = pick(NATURE, nature)
  return known ? t(known) : nature
}
export const NATURES = Object.keys(NATURE)

const UNIT: Record<string, string> = { min: tx('per minute'), img: tx('per image'), Mtok: tx('per M tokens') }
const money = (value: number, currency = '$') =>
  value === 0 ? t('free') : `${value.toLocaleString(currentLang(), { minimumFractionDigits: 2, maximumFractionDigits: 3 })} ${currency}`

/** The price of a model: input and output per million tokens, or a price per unit. Never made up. */
export function Price({ model }: { model: CatalogModel }) {
  const approx = model.estimated ? <span title={t('Estimated price: to check with the provider')}>≈ </span> : null
  if (model.price_in !== null && model.price_out !== null) {
    if (model.price_in === 0 && model.price_out === 0) return <span>{t('free')}</span>
    return (
      <span className="num whitespace-nowrap">
        {approx}
        {money(model.price_in)} <span className="text-muted-foreground">/</span> {money(model.price_out)}
      </span>
    )
  }
  if (model.unit_price !== null) {
    if (model.unit_price === 0) return <span>{t('free')}</span>
    const unit = pick(UNIT, model.unit)
    return (
      <span className="num whitespace-nowrap">
        {approx}
        {money(model.unit_price, model.currency ?? '$')} <span className="font-sans text-muted-foreground">{unit ? t(unit) : t('per {unit}', { unit: String(model.unit) })}</span>
      </span>
    )
  }
  return <span className="text-muted-foreground">{t('not specified')}</span>
}

/** Connected or not: the only place on these screens where the green signal lights up. */
export function Connection({ on, className }: { on: boolean; className?: string }) {
  return (
    <span className={cn('inline-flex items-center gap-1.5 whitespace-nowrap', !on && 'text-muted-foreground', className)}>
      <span className={cn('size-2 rounded-full', on ? 'bg-st-working' : 'ring-1 ring-st-idle ring-inset')} aria-hidden />
      {on ? t('Connected') : t('Not connected')}
    </span>
  )
}
