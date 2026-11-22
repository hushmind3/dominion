import {useState} from 'react'
import {Button,TextInput} from './Primitives'
import {KeyRound,LogIn,RotateCcw,X} from 'lucide-react'
import {StatusPill} from './StatusPill'
import {t,useLocale,type LocaleKey} from '../i18n'

export type CliAuthStatus={
 source:string
 status:string
 message:string
 url?:string|null
 userCode?:string|null
 requiresInput?:boolean
}
type Props={
 session:CliAuthStatus
 onInput:(value:string)=>Promise<void>
 onCancel:()=>Promise<void>
}

/** Browser authorization progress stays inside DOMINION, even when CLI asks for a code/menu. */
export function CliAuthProgress({session,onInput,onCancel}:Props){
 useLocale()
 const [code,setCode]=useState('')
 const active=!['completed','failed','cancelled'].includes(session.status)
 const title:Record<string,string>={claude:'Claude Code',gemini:'Gemini CLI',opencode:'OpenCode',google_workspace:'Google Workspace'}
 const statuses:Record<string,LocaleKey>={
  starting:'cliAuth.status.starting',browser:'cliAuth.status.browser',input:'cliAuth.status.input',
  completed:'cliAuth.status.completed',failed:'cliAuth.status.failed',cancelled:'cliAuth.status.cancelled',
 }
 const message=session.status==='starting'?t('cliAuth.message.starting'):
  session.status==='browser'?session.userCode?t('cliAuth.message.enterCode',{code:session.userCode}):t('cliAuth.message.browser'):
  session.status==='input'?t('cliAuth.message.input'):
  session.status==='completed'?t('cliAuth.message.completed'):
  session.message||t('cliAuth.message.failed')
 return <section className="mt-4 min-w-0 rounded-xl border border-[#D7D9E9] bg-[#F7F8FC] p-4" aria-live="polite">
  <div className="flex flex-wrap items-start justify-between gap-2">
   <div className="min-w-0">
    <strong className="block text-[13px] text-[#363650]">{t('cliAuth.title',{source:title[session.source]||session.source})}</strong>
    <p className="mt-1 text-[12px] leading-5 text-[#626278]">{message}</p>
   </div>
   <div className="flex items-center gap-2"><StatusPill status={session.status}>{t(statuses[session.status]||'cliAuth.status.unknown')}</StatusPill>
    {active&&<Button size="sm" variant="ghost" onClick={()=>void onCancel()}><X size={13}/>{t('cliAuth.cancel')}</Button>}</div>
  </div>
  {session.url&&<a href={session.url} target="_blank" rel="noopener noreferrer"
    className="mt-3 inline-flex min-h-9 items-center gap-2 rounded-lg bg-brand px-3 text-[12px] font-semibold text-white hover:bg-brand-hover"><LogIn size={14}/>{t('cliAuth.openLogin')}</a>}
  {session.userCode&&<div className="mt-3 rounded-lg border border-[#E2E2EE] bg-white px-3 py-2"><span className="block text-[10px] text-[#626278]">{t('cliAuth.userCode')}</span><code className="select-all font-mono text-lg font-bold tracking-widest text-[#363650]">{session.userCode}</code></div>}
  {active&&session.requiresInput&&<div className="mt-3 flex flex-wrap items-center gap-2 border-t border-[#E2E2EE] pt-3">
   <Button size="sm" variant="secondary" onClick={()=>void onInput('enter')}><RotateCcw size={13}/>{t('cliAuth.continue')}</Button>
   <Button size="sm" variant="ghost" onClick={()=>void onInput('down')}>{t('cliAuth.nextOption')}</Button>
   <div className="flex min-w-[180px] flex-1 items-center gap-2">
    <TextInput aria-label={t('cliAuth.codeLabel')} value={code} onChange={e=>setCode(e.target.value)} placeholder={t('cliAuth.codePlaceholder')} className="!py-1.5 !text-[12px]"/>
    <Button size="sm" variant="secondary" disabled={!code.trim()} onClick={()=>{void onInput(code.trim());setCode('')}}><KeyRound size={13}/>{t('cliAuth.submit')}</Button>
   </div>
  </div>}
  <p className="mt-2 text-[11px] leading-5 text-[#8D8D9B]">{t('cliAuth.securityNote')}</p>
 </section>
}
