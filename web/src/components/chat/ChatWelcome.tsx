import {ArrowRight,FileText,Lightbulb,MessageSquare,Sparkles} from 'lucide-react'
import {Button} from '../Primitives'
import {t,useLocale} from '../../i18n'

type Props={
 ready:boolean;loading:boolean;error?:string;onRetry:()=>void
 onConnect:()=>void;onPrompt:(text:string)=>void
}

/** The conversation landing state is an actionable starting point, not a blank page. */
export function ChatWelcome({ready,loading,error,onRetry,onConnect,onPrompt}:Props){
 useLocale()
 const prompts=[
  {icon:Lightbulb,title:'welcome.idea',prompt:'welcome.ideaPrompt'},
  {icon:FileText,title:'welcome.write',prompt:'welcome.writePrompt'},
  {icon:MessageSquare,title:'welcome.think',prompt:'welcome.thinkPrompt'},
 ] as const
 return <section className="chat-welcome mx-auto flex min-h-full w-full max-w-[860px] flex-col items-center justify-center px-5 py-10 text-center sm:px-8 sm:py-16" aria-label={t('welcome.title')}>
  <div className="mb-5 grid size-[60px] place-items-center rounded-[20px] border border-[#E9D5C3] bg-gradient-to-br from-[#F9EDDF] to-[#F3DCCD] text-[#B86E4D] shadow-sm" aria-hidden="true"><Sparkles size={28} strokeWidth={1.5}/></div>
  <span className="text-[11px] font-semibold tracking-[.16em] text-[#A18A72]">{t('welcome.eyebrow')}</span>
  <h1 className="mt-3 max-w-[620px] text-balance text-[clamp(25px,3.1vw,36px)] font-semibold leading-[1.5] tracking-[-.06em] text-[#3A3229]">{t('welcome.title')}</h1>
  <p className="mt-3 max-w-md text-balance text-[13px] leading-7 text-[#918372] sm:text-sm">{t('welcome.description')}</p>
  {error&&<div className="mt-6 flex w-full max-w-lg flex-wrap items-center justify-between gap-4 rounded-2xl border border-danger/20 bg-danger-soft p-4 text-left" role="alert">
   <div className="min-w-0 flex-1"><strong className="text-sm text-ink">{t('welcome.connectionLost')}</strong><p className="mt-1 text-xs text-danger">{error}</p></div>
   <Button variant="secondary" onClick={onRetry}>{t('history.retry')}</Button>
  </div>}
  {!ready&&!loading&&!error&&<div className="mt-7 flex w-full max-w-lg flex-col items-start justify-between gap-4 rounded-2xl border border-[#E7C6AE] bg-[#FAF0E3] p-5 text-left sm:flex-row sm:items-center">
   <div className="min-w-0 flex-1"><strong className="text-sm text-[#654E38]">{t('welcome.connectTitle')}</strong><p className="mt-2 text-xs leading-6 text-[#95806A]">{t('welcome.connectDescription')}</p></div>
   <Button variant="primary" onClick={onConnect} className="!shrink-0">{t('welcome.connect')}<ArrowRight size={16}/></Button>
  </div>}
  {loading&&<p className="mt-5 text-sm text-muted" role="status">{t('welcome.loading')}</p>}
  <div className="mt-9 grid w-full grid-cols-1 gap-3 text-left sm:grid-cols-3" aria-label={t('chat.examples')}>
   {prompts.map(({icon:Icon,title,prompt})=><Button key={title} variant="bare" onClick={()=>onPrompt(t(prompt))}
    className="group relative !flex min-h-[144px] !w-full !flex-col !items-start !justify-start !gap-0 !rounded-2xl !border !border-[#E8E0D4] !bg-[#FDFAF4] !px-4 !py-5 !text-left transition-all hover:!-translate-y-0.5 hover:!border-[#C9A185] hover:!bg-[#FAF3E8] focus-visible:!ring-2 focus-visible:!ring-brand sm:!min-h-[162px]">
    <Icon size={19} strokeWidth={1.6} className="mb-4 text-[#A28368]"/>
    <strong className="text-[13px] font-semibold text-[#554536]">{t(title)}</strong>
    <span className="mt-2 line-clamp-3 text-[11px] font-normal leading-[1.7] text-[#978978]">{t(prompt)}</span>
    <ArrowRight size={15} className="absolute right-4 top-5 text-[#B9A590] transition-transform group-hover:translate-x-1"/>
   </Button>)}
  </div>
  <p className="mt-6 text-[11px] text-[#A59888]">{t('welcome.note')}</p>
 </section>
}
