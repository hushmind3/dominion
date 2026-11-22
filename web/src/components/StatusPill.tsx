import type {ReactNode} from 'react'
import {t,useLocale,type LocaleKey} from '../i18n'
import {plainStatus} from '../i18n/plain'

const labels:Record<string,LocaleKey>={queued:'st.queued',planning:'st.planning',running:'st.running',reviewing:'st.reviewing',repairing:'st.repairing',completed:'st.completed',failed:'st.failed',needs_review:'st.needs_review',budget_exceeded:'st.budget_exceeded',cancelled:'st.cancelled',pending:'st.pending',available:'st.available',ready:'st.ready',answered_unverified:'st.answered_unverified',answered_verified:'st.answered_verified'}
export function StatusPill({status,children}:{status:string;children?:ReactNode}){
 useLocale()
 const tone=['completed','ready','available'].includes(status)?'bg-success-soft text-success ring-success/20':
  ['failed','budget_exceeded'].includes(status)?'bg-danger-soft text-danger ring-danger/20':
  ['needs_review','repairing'].includes(status)?'bg-warning-soft text-warning ring-warning/20':
  ['running','reviewing','planning'].includes(status)?'bg-brand-soft text-brand-hover ring-brand-soft':'bg-canvas text-muted ring-divider';
 return <span className={'inline-flex items-center gap-1.5 rounded-lg px-2.5 py-1 text-[11px] font-semibold ring-1 ring-inset '+tone}>
  {children??(labels[status]?t(labels[status]):plainStatus(status))}
 </span>
}

export function statusLabel(status?: string): string {
  if (!status) return ''
  return labels[status] ? t(labels[status]) : plainStatus(status)
}
