import {ArrowLeft,BookOpenText,CheckCircle2,RadioTower,Settings2,Workflow,X} from 'lucide-react'
import {DialogBackdrop} from './Dialog'
import {WorkspacePage} from './WorkspacePage'
import {Button} from './Primitives'
import {settingsPages,type PageId,type SettingsPage} from '../workspacePages'
import {t,useLocale} from '../i18n'

/** Sub4-style management workspace: separate from the live conversation.
 * The four destinations own their content; no duplicate tool/market panels. */
const sections=[
 {id:'settings' as const,ko:'설정',en:'Settings',icon:Settings2},
 {id:'connections' as const,ko:'서비스 연결',en:'Service connections',icon:RadioTower},
 {id:'memory' as const,ko:'기억·지침',en:'Memory & prompts',icon:BookOpenText},
 {id:'tasks' as const,ko:'작업',en:'Tasks',icon:Workflow},
]

export function SettingsWorkspace({page,onNavigate,onClose,conversationSettingsRef,online}:{
 page:SettingsPage
 onNavigate:(page:PageId,sessionId?:string)=>void
 onClose:()=>void
 conversationSettingsRef:(el:HTMLDivElement|null)=>void
 online:boolean
}){
 const en=useLocale()==='en'
 const current=settingsPages[page]
 return <DialogBackdrop onDismiss={onClose}>
  <div role="dialog" aria-modal="true" aria-label={en?'DOMINION settings':'DOMINION 설정'}
   className="flex h-[min(940px,calc(100dvh-32px))] w-full max-w-[1440px] min-w-0 overflow-hidden rounded-[22px] border border-divider bg-surface shadow-[0_24px_100px_rgba(41,35,28,.18)] max-md:h-[100dvh] max-md:max-h-none max-md:rounded-none max-md:border-0">
   <aside className="hidden w-[240px] shrink-0 flex-col border-r border-divider bg-[#F4F1EA] md:flex" aria-label={en?'Settings categories':'설정 분류'}>
    <div className="flex min-h-[86px] items-center gap-3 px-5">
     <span className="grid size-9 shrink-0 place-items-center rounded-xl border border-[#C9644230] bg-[#F5E5DA] text-xl font-extrabold text-brand">D</span>
     <strong className="text-xl tracking-[-.05em] text-ink">dominion</strong>
    </div>
    <nav className="min-h-0 flex-1 space-y-1 overflow-y-auto px-3 pb-6" aria-label={en?'Management navigation':'관리 메뉴'}>
     <p className="px-3 pb-2 text-[11px] font-medium text-[#9B8B78]">{en?'MANAGEMENT':'관리'}</p>
     {sections.map(({id,ko,en:english,icon:Icon})=><Button key={id} variant="bare"
      aria-current={page===id?'page':undefined} onClick={()=>onNavigate(id)}
      className={'flex min-h-11 w-full items-center justify-start gap-3 rounded-xl px-3 py-2 text-left text-[13px] hover:bg-[#EBE4D8] '+(page===id?'bg-[#E8DFD0] font-semibold text-[#554333]':'text-[#796B5B]')}>
      <Icon size={18} strokeWidth={1.65}/><span>{en?english:ko}</span>
     </Button>)}
    </nav>
    <div className="flex items-center gap-2 border-t border-divider px-5 py-4 text-xs text-muted">
     <span className={'size-2 rounded-full '+(online?'bg-success':'bg-warning')}/>
     {online?(en?'Connected':'연결됨'):(en?'Disconnected':'연결 끊김')}
    </div>
   </aside>
   <div className="flex min-w-0 flex-1 flex-col">
    <header className="flex min-h-[108px] shrink-0 items-center justify-between gap-3 border-b border-divider bg-surface px-5 py-4 md:px-8">
     <div className="min-w-0 flex-1">
      <p className="text-[11px] font-semibold tracking-wider text-[#A9957D]">{en?'DOMINION / MANAGEMENT':'DOMINION / 관리'}</p>
      <h1 className="mt-1 text-xl font-semibold text-[#46382C]">{t(current.label)}</h1>
      <p className="mt-1 hidden text-xs text-muted sm:block">{t(current.description)}</p>
     </div>
     <Button variant="secondary" onClick={onClose} className="!shrink-0" aria-label={en?'Back to conversation':'대화로 돌아가기'}>
      <ArrowLeft size={17}/><span className="hidden sm:inline">{en?'Back to chat':'대화로 돌아가기'}</span>
      <X size={16} className="sm:hidden"/>
     </Button>
    </header>
    <nav className="flex shrink-0 gap-1 overflow-x-auto border-b border-divider bg-[#FBF9F4] px-3 py-2 md:hidden" aria-label={en?'Management navigation':'관리 메뉴'}>
     {sections.map(({id,ko,en:english,icon:Icon})=><Button key={id} variant="bare" onClick={()=>onNavigate(id)} aria-current={page===id?'page':undefined}
      className={'!shrink-0 !gap-1.5 !rounded-lg !px-3 !py-2 !text-xs '+(page===id?'!bg-brand-soft !font-semibold !text-brand-hover':'!text-muted')}>
      <Icon size={15}/>{en?english:ko}</Button>)}
    </nav>
    <div className="min-h-0 flex-1 overflow-hidden bg-[#FBF9F4]">
     <WorkspacePage embedded page={page} onNavigate={onNavigate} conversationSettingsRef={conversationSettingsRef}/>
    </div>
   </div>
  </div>
 </DialogBackdrop>
}
