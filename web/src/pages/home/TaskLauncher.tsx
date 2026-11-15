import {useRef,useState,type FormEvent} from 'react'
import {ArrowRight,BookOpenText,Clapperboard,Code2,Cpu,MessageSquare,UsersRound} from 'lucide-react'
import {post} from '../../api'
import {Button,TextArea} from '../../components/Primitives'
import {createGoalPayload,DEFAULT_GOAL_BUDGET,type GoalStartKind} from './goalStart.mjs'

type Destination='chat'|'goals'|'models'|'video'|'rooms'
type Props={navigate:(target:Destination)=>void;modelConnected:boolean;modelLoading:boolean}
const destinations=[
 {page:'chat',title:'바로 대화하기',description:'질문하고 함께 생각하기',icon:MessageSquare},
 {page:'video',title:'영상 만들기',description:'장면·음성·결과물 제작',icon:Clapperboard},
 {page:'rooms',title:'여러 AI에게 묻기',description:'토론하고 의견 비교하기',icon:UsersRound},
] as const

/** A human-readable entry to the actual durable Rust /api/goals queue. */
export function TaskLauncher({navigate,modelConnected,modelLoading}:Props){
 const [kind,setKind]=useState<GoalStartKind>('general')
 const [prompt,setPrompt]=useState('')
 const [tokenBudget,setTokenBudget]=useState(DEFAULT_GOAL_BUDGET)
 const [busy,setBusy]=useState(false)
 const [error,setError]=useState('')
 const submitting=useRef(false)
 const start=async(event:FormEvent<HTMLFormElement>)=>{
  event.preventDefault()
  if(submitting.current)return
  let payload
  try{payload=createGoalPayload(prompt,kind,tokenBudget)}
  catch(cause){setError((cause as Error).message);return}
  submitting.current=true
  setBusy(true);setError('')
  try{
   const created=await post<{id:string}>('/api/goals',payload)
   if(!created?.id)throw new Error('서버가 작업 번호를 돌려주지 않았습니다.')
   setPrompt('')
   window.dispatchEvent(new Event('dominion-refresh'))
   navigate('goals')
  }catch(cause){
   // POSTs are not retried automatically. The server may have received one;
   // let the owner inspect the durable queue before repeating it.
   setError('작업 등록 결과를 확인하지 못했습니다. 실행 기록에 이미 등록됐는지 먼저 확인해 주세요. '+(cause as Error).message)
  }finally{submitting.current=false;setBusy(false)}
 }
 return <section aria-label="원하는 작업 시작" className="mb-5 overflow-hidden rounded-2xl border border-[#E7DCCF] bg-white shadow-[0_8px_28px_rgba(66,49,29,.05)]">
  <div className="border-b border-[#EFE8DD] bg-[#FAF7F1] px-5 py-5 sm:px-7">
   <p className="mb-1 text-xs font-bold tracking-wide text-[#A94E30]">DOMINION · 작업 시작</p>
   <h2 className="text-xl font-bold leading-snug tracking-tight text-[#29231C] sm:text-2xl">어떤 일을 맡기시겠어요?</h2>
   <p className="mt-2 text-sm leading-6 text-[#746A5E]">전문 용어나 복잡한 설정 없이, 원하는 결과를 편하게 적으시면 됩니다.</p>
  </div>
  <form onSubmit={start} className="space-y-4 px-5 py-5 sm:px-7">
   <div role="group" aria-label="맡길 작업 종류" className="grid grid-cols-2 gap-2">
    <Button variant="bare" type="button" aria-pressed={kind==='general'}
     onClick={()=>setKind('general')}
     className={'flex min-h-12 items-center justify-center gap-2 rounded-xl border px-3 py-3 text-sm font-semibold '+
      (kind==='general'?'border-[#C96442] bg-[#FFF4EB] text-[#96432B]':'border-[#E7E0D5] bg-white text-[#645B51] hover:bg-[#FAF7F1]')}>
     <BookOpenText size={17}/> 자료 조사·일반 작업
    </Button>
    <Button variant="bare" type="button" aria-pressed={kind==='code'}
     onClick={()=>setKind('code')}
     className={'flex min-h-12 items-center justify-center gap-2 rounded-xl border px-3 py-3 text-sm font-semibold '+
      (kind==='code'?'border-[#C96442] bg-[#FFF4EB] text-[#96432B]':'border-[#E7E0D5] bg-white text-[#645B51] hover:bg-[#FAF7F1]')}>
     <Code2 size={17}/> 프로그램·파일 수정
    </Button>
   </div>
   <label htmlFor="home-task-input" className="block text-sm font-semibold text-[#3A342D]">원하는 결과</label>
   <TextArea id="home-task-input" name="goal" value={prompt} disabled={busy}
    rows={4} maxLength={16000} onChange={e=>setPrompt(e.target.value)}
    className="!text-[15px] !leading-7"
    placeholder={kind==='code'?'예: 로그인 오류를 찾아 수정하고, 테스트로 확인한 뒤 바뀐 파일을 알려줘':'예: 최신 자료를 조사하고 핵심 내용을 정리해서 보고서를 만들어줘'}/>
   <p className="text-xs leading-5 text-[#7C7469]">
    {kind==='code'?'코드 작업은 별도의 Git 작업공간과 실제 테스트 결과를 요구합니다. Git 연결이 필요할 수 있습니다.':
    '조사와 일반 작업도 실행 과정·오류·결과를 기록하며 진행합니다.'}
   </p>
   <details className="rounded-xl border border-[#EEE6DA] px-4 py-3 text-sm text-[#60564B]">
    <summary className="cursor-pointer font-medium">실행 사용량 상한 조절 (선택)</summary>
    <div className="mt-3 flex flex-wrap items-center gap-3">
     <label htmlFor="home-task-budget" className="text-xs">전체 작업의 최대 토큰 수</label>
     <input id="home-task-budget" type="number" min={1000} max={2000000} step={1000}
      value={tokenBudget} disabled={busy} onChange={e=>setTokenBudget(Number(e.target.value))}
      className="w-36 rounded-lg border border-[#E1D9CC] px-3 py-2 text-sm"/>
     <p className="w-full text-xs text-[#8A8176]">모델과 하위 에이전트 사용량을 합친 상한입니다. 실제 원화 요금은 선택한 모델에 따라 달라집니다.</p>
    </div>
   </details>
   {error&&<div role="alert" className="rounded-xl border border-rose-200 bg-rose-50 px-4 py-3 text-sm leading-6 text-rose-800">
    {error} <Button variant="ghost" onClick={()=>navigate('goals')} className="ml-1 underline">실행 기록 보기</Button>
   </div>}
   <div className="flex flex-wrap items-center justify-between gap-3">
    <div className="flex items-start gap-2 text-xs leading-5 text-[#6E655B]">
     <Cpu size={16} className="mt-0.5 shrink-0"/>
     <span>{modelLoading?'연결된 모델 확인 중…':modelConnected?'실행할 모델이 연결돼 있습니다.':'사용할 모델이 아직 확인되지 않았습니다.'}
      {!modelLoading&&!modelConnected&&<Button variant="ghost" onClick={()=>navigate('models')} className="ml-1 underline">모델 연결하기</Button>}
     </span>
    </div>
    <Button type="submit" variant="primary" disabled={busy||!prompt.trim()||!Number.isSafeInteger(tokenBudget)||tokenBudget<1000||tokenBudget>2000000}
     className="!min-h-11 !px-5 !text-sm">
     {busy?'작업 등록 중…':'이 작업 맡기기'}<ArrowRight size={16}/>
    </Button>
   </div>
  </form>
  <div className="border-t border-[#EFE8DD] bg-[#FFFDF9] px-5 py-4 sm:px-7">
   <p className="mb-3 text-sm font-semibold">다른 방식으로 시작하기</p>
   <div className="grid gap-2 sm:grid-cols-3">
    {destinations.map(item=>{
     const Icon=item.icon
     return <Button key={item.page} variant="bare" onClick={()=>navigate(item.page)}
      className="flex min-h-[66px] items-center gap-3 rounded-xl border border-[#E9E1D6] bg-white px-4 py-3 text-left hover:bg-[#FAF7F1]">
      <Icon size={20} className="shrink-0 text-[#C96442]"/>
      <span className="min-w-0 flex-1"><span className="block text-sm font-semibold">{item.title}</span>
       <span className="block text-xs leading-5 text-[#8A8176]">{item.description}</span></span>
      <ArrowRight size={15} className="shrink-0 text-[#9C9388]"/>
     </Button>
    })}
   </div>
  </div>
 </section>
}
