import type {ReactNode} from 'react'
import {t,useLocale,type LocaleKey} from '../i18n'
import {plainStatus} from '../i18n/plain'

const labels:Record<string,LocaleKey>={queued:'st.queued',planning:'st.planning',running:'st.running',reviewing:'st.reviewing',repairing:'st.repairing',preparing:'st.preparing',resolving:'st.resolving',checking:'st.checking',downloading:'st.downloading',installing:'st.installing',extracting:'st.extracting',installed:'st.installed',already_installed:'st.already_installed',applying:'st.applying',applied:'st.applied',completed:'st.completed',succeeded:'st.succeeded',success:'st.success',failed:'st.failed',failure:'st.failure',unknown:'st.unknown',needs_review:'st.needs_review',awaiting_approval:'st.awaiting_approval',interrupted:'em.status.interrupted',budget_exceeded:'st.budget_exceeded',cancelled:'st.cancelled',paused:'st.paused',pending:'st.pending',available:'st.available',ready:'st.ready',active:'st.active',candidate:'st.candidate',needs_split:'st.needs_split',archived:'st.archived',disabled:'st.disabled',unrated:'st.unrated',excluded:'st.excluded',verifying:'st.verifying',answered_unverified:'st.answered_unverified',answered_verified:'st.answered_verified'}
export function StatusPill({status,children}:{status:string;children?:ReactNode}){
 useLocale()
 const tone=['completed','succeeded','success','ready','available','applied','active'].includes(status)?'bg-emerald-50 text-emerald-700 ring-emerald-100':
  ['failed','failure','budget_exceeded'].includes(status)?'bg-rose-50 text-rose-700 ring-rose-100':
  ['needs_review','repairing','awaiting_approval','interrupted','paused','candidate','needs_split','verifying','browser','input'].includes(status)?'bg-amber-50 text-amber-800 ring-amber-100':
  ['running','reviewing','planning','preparing','resolving','checking','downloading','installing','extracting','applying','starting'].includes(status)?'bg-brand-soft text-brand-hover ring-brand-soft':'bg-canvas text-muted ring-divider';
 return <span className={'inline-flex items-center gap-1.5 rounded-lg px-2.5 py-1 text-[11px] font-semibold ring-1 ring-inset '+tone}>
  {children??(labels[status]?t(labels[status]):plainStatus(status))}
 </span>
}

export function statusLabel(status?: string): string {
  if (!status) return ''
  return labels[status] ? t(labels[status]) : plainStatus(status)
}
