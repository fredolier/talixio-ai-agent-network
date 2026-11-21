import { cn, pick } from '@/lib/utils'
import type { CatalogModel, CatalogProvider } from '@/lib/types'

const TRUST: Record<string, { label: string; className: string }> = {
  '1': { label: 'grand public', className: 'border-st-off/60 text-st-off' },
  '2': { label: 'gratuit ou public', className: 'border-st-wait/70 text-foreground' },
  '3': { label: 'professionnel', className: 'border-border text-foreground' },
  '4': { label: 'sans conservation', className: 'border-foreground/40 text-foreground' },
  '5': { label: 'souverain (RGPD)', className: 'border-foreground bg-foreground text-background' },
  L: { label: 'local', className: 'border-foreground bg-foreground text-background' },
}

/** Les niveaux de confiance, pour la légende. */
export const TRUST_LEVELS = Object.entries(TRUST).map(([level, { label }]) => ({ level, label }))

/** Le niveau de confiance d'un fournisseur : un chiffre dans un carré, et ce qu'il veut dire. */
export function Trust({ level, withLabel }: { level: string; withLabel?: boolean }) {
  const known = pick(TRUST, level)
  if (!known) return <span className="text-muted-foreground">non évalué</span>
  return (
    <span className="inline-flex items-center gap-2 whitespace-nowrap">
      <span className={cn('num grid size-5 place-items-center rounded-[5px] border text-[11px] font-medium', known.className)}>{level}</span>
      {withLabel && <span className="text-[13px]">{known.label}</span>}
    </span>
  )
}

/** Européen, hors d'Europe, ou sur votre propre machine. */
export function Origin({ provider }: { provider: CatalogProvider }) {
  if (provider.trust === 'L') return <span>Local</span>
  if (provider.europe === null) return <span className="text-muted-foreground">non renseigné</span>
  return provider.europe ? <span className="font-medium">Europe</span> : <span className="text-muted-foreground">Hors d'Europe</span>
}

/** La qualité relative d'un modèle, de 1 à 10 : dix crans, pas un dégradé. */
export function Quality({ rank }: { rank: number | null }) {
  if (rank === null) return <span className="text-muted-foreground">non classé</span>
  return (
    <span className="inline-flex items-center gap-2" title={`Qualité ${rank} sur 10`}>
      <span className="flex gap-px" aria-hidden>
        {Array.from({ length: 10 }, (_, i) => (
          <span key={i} className={cn('h-3 w-[5px] rounded-[1px]', i < rank ? 'bg-foreground' : 'bg-border')} />
        ))}
      </span>
      <span className="num w-4 text-right text-[13px]">{rank}</span>
    </span>
  )
}

const NATURE: Record<string, string> = { LLM: 'Texte', Vision: 'Vision', Voice: 'Voix', Image: 'Image', Embedding: 'Plongements', Forecast: 'Prévision', Decision: 'Décision' }
export const natureLabel = (nature: string) => pick(NATURE, nature) ?? nature
export const NATURES = Object.keys(NATURE)

const UNIT: Record<string, string> = { min: 'minute', img: 'image', Mtok: 'M tokens' }
const money = (value: number, currency = '$') =>
  value === 0 ? 'gratuit' : `${value.toLocaleString('fr-FR', { minimumFractionDigits: 2, maximumFractionDigits: 3 })} ${currency}`

/** Le prix d'un modèle : entrée et sortie par million de tokens, ou prix à l'unité. Jamais inventé. */
export function Price({ model }: { model: CatalogModel }) {
  const approx = model.estimated ? <span title="Prix estimé : à vérifier chez le fournisseur">≈ </span> : null
  if (model.price_in !== null && model.price_out !== null) {
    if (model.price_in === 0 && model.price_out === 0) return <span>gratuit</span>
    return (
      <span className="num whitespace-nowrap">
        {approx}
        {money(model.price_in)} <span className="text-muted-foreground">/</span> {money(model.price_out)}
      </span>
    )
  }
  if (model.unit_price !== null) {
    if (model.unit_price === 0) return <span>gratuit</span>
    return (
      <span className="num whitespace-nowrap">
        {approx}
        {money(model.unit_price, model.currency ?? '$')} <span className="font-sans text-muted-foreground">par {pick(UNIT, model.unit) ?? model.unit}</span>
      </span>
    )
  }
  return <span className="text-muted-foreground">non renseigné</span>
}

/** Connecté ou non : le seul endroit de ces écrans où le signal vert s'allume. */
export function Connection({ on, className }: { on: boolean; className?: string }) {
  return (
    <span className={cn('inline-flex items-center gap-1.5 whitespace-nowrap', !on && 'text-muted-foreground', className)}>
      <span className={cn('size-2 rounded-full', on ? 'bg-st-working' : 'ring-1 ring-st-idle ring-inset')} aria-hidden />
      {on ? 'Connecté' : 'Non connecté'}
    </span>
  )
}
