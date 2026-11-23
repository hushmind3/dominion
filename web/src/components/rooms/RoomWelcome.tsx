import {ArrowRight,Menu,Plus,UsersRound} from 'lucide-react'
import type {Agent} from '../../api'
import {t,useLocale} from '../../i18n'
import {Button} from '../Primitives'
import {RoomAvatar} from './RoomAvatar'

/** Room onboarding mirrors the conversational welcome, without mixing setup controls into the chat. */
export function RoomWelcome({hasAgents,onCreate,onOpenNavigation}:{hasAgents:boolean;onCreate:()=>void;onOpenNavigation?:()=>void}){
 const en=useLocale()==='en'
 const tr=(ko:string,english:string)=>en?english:ko
 return <section className="relative flex min-h-0 w-full flex-1 flex-col items-center justify-center overflow-y-auto bg-surface px-5 py-10 text-center sm:px-8" aria-label={t('rooms.welcome.title')}>
  <Button variant="ghost" size="icon" className="absolute left-3 top-3 md:hidden" aria-label={t('rooms.listExpand')} onClick={onOpenNavigation}><Menu size={20}/></Button>
  <div className="mb-5 grid size-[60px] place-items-center rounded-[20px] border border-[#E9D5C3] bg-gradient-to-br from-[#F9EDDF] to-[#F3DCCD] text-[#B86E4D]" aria-hidden="true"><UsersRound size={27} strokeWidth={1.5}/></div>
  <span className="text-[11px] font-semibold tracking-[.16em] text-[#A18A72]">{tr('함께 만드는 대화 공간','YOUR TEAM WORKSPACE')}</span>
  <h1 className="mt-3 max-w-lg text-balance text-[clamp(25px,3vw,36px)] font-semibold leading-[1.45] tracking-[-.055em] text-[#3A3229]">{t('rooms.welcome.title')}</h1>
  <p className="mt-3 max-w-md text-balance text-sm leading-7 text-[#918372]">{t('rooms.welcome.line1')} {t('rooms.welcome.line2')} {t('rooms.welcome.line3')}</p>
  <div className="mt-8 flex w-full max-w-lg flex-col gap-4 rounded-2xl border border-[#E8E0D4] bg-[#FDFAF4] p-5 text-left sm:flex-row sm:items-center sm:justify-between sm:p-6">
   <div className="min-w-0 flex-1">
    <strong className="block text-sm font-semibold text-[#554536]">{hasAgents?tr('함께할 AI를 고르세요','Choose your teammates'):tr('먼저 AI 페르소나를 준비하세요','Set up your AI personas')}</strong>
    <p className="mt-2 text-xs leading-6 text-[#978978]">{hasAgents?tr('참여할 AI를 정하고 대화방을 만들면 바로 시작합니다.','Select participants, create a room, and start talking.'):tr('기억 메뉴의 페르소나에서 AI를 만든 뒤 이곳에서 그룹을 구성하세요.','Create an AI persona in Memory, then bring it into your group.')}</p>
   </div>
   {hasAgents?<Button variant="primary" className="!shrink-0" onClick={onCreate}><Plus size={17}/>{tr('대화방 만들기','Create room')}<ArrowRight size={16}/></Button>
   :<a href="#/memory?view=personas" className="inline-flex min-h-11 shrink-0 items-center justify-center gap-2 rounded-xl bg-brand px-4 py-2 text-sm font-semibold text-white hover:bg-brand-hover"><Plus size={16}/>{tr('페르소나 만들기','Create persona')}<ArrowRight size={15}/></a>}
  </div>
 </section>
}

export function RoomFirstMessage({roster}:{roster:Agent[]}){
 const en=useLocale()==='en'
 return <div className="flex h-full min-h-60 flex-col items-center justify-center px-5 py-10 text-center">
  <div className="mb-6 flex gap-2 rounded-2xl border border-divider bg-[#FDFAF4] p-3"><RoomAvatar owner/>{roster.slice(0,4).map(agent=><RoomAvatar key={agent.id} agent={agent}/>)}</div>
  <h1 className="text-2xl font-semibold tracking-[-.04em] text-[#3A3229]">{t('rooms.firstTitle')}</h1>
  <p className="mt-3 max-w-md text-sm leading-7 text-[#918372]">{en?'Write a message to everyone, or choose a specific AI to answer.':'아래에서 모두에게 메시지를 보내거나 답변할 AI를 골라 대화를 시작하세요.'}</p>
 </div>
}
