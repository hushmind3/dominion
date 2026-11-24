import {Plus, UsersRound} from 'lucide-react'
import type {Agent} from '../../api'
import {t, useLocale} from '../../i18n'
import {Button} from '../Primitives'
import {RoomAvatar} from './RoomAvatar'

export function RoomWelcome({hasAgents,onCreate}:{hasAgents:boolean;onCreate:()=>void}) {
 const en=useLocale()==='en'
 const tr=(ko:string,english:string)=>en?english:ko
 return <div className="relative flex min-h-0 flex-1 flex-col items-center justify-center gap-5 overflow-y-auto bg-surface px-6 py-12 text-center">
  <span className="grid size-14 place-items-center rounded-2xl border border-divider bg-canvas text-brand"><UsersRound size={28}/></span>
  <h1 className="text-3xl font-semibold tracking-tight">{tr('함께 이야기할 AI를 고르세요','Choose the AIs to talk with')}</h1>
  <p className="max-w-md text-sm leading-7 text-muted">{tr('한곳에서 여러 AI와 대화하고, 필요할 때 함께할 일을 맡길 수 있습니다.','Talk with several AIs in one place and assign collaborative work when needed.')}</p>
  {hasAgents?<Button variant="primary" onClick={onCreate}><Plus size={18}/>{tr('새 그룹 만들기','Create a group')}</Button>:<><p className="max-w-md text-sm leading-7 text-muted">{tr('먼저 그룹에 참여할 AI와 페르소나를 만들어 주세요.','Create the AIs and personas that will join your group first.')}</p><a href="#/agents" className="inline-flex min-h-11 items-center gap-2 rounded-xl bg-brand px-4 py-2 text-sm font-semibold text-white hover:bg-brand-hover"><Plus size={18}/>{tr('참여할 AI 만들기','Create an AI')}</a></>}
 </div>
}

export function RoomFirstMessage({roster}:{roster:Agent[]}) {
 const en=useLocale()==='en'
 return <div className="flex h-full min-h-60 flex-col items-center justify-center px-5 py-8 text-center">
  <div className="mb-5 flex gap-2"><RoomAvatar owner/>{roster.slice(0,3).map(agent=><RoomAvatar key={agent.id} agent={agent}/>)}</div>
  <h1 className="text-2xl font-semibold tracking-tight">{t('rooms.firstTitle')}</h1>
  <p className="mt-3 max-w-md text-sm leading-7 text-muted">{en?'Write a message below. Choose a recipient when you want a particular AI to answer.':'아래에 메시지를 보내세요. 특정 AI의 답변을 원하면 입력창에서 답변할 상대를 선택할 수 있습니다.'}</p>
 </div>
}
