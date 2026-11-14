import {useEffect,useState} from 'react'
import {GitMerge,RefreshCw,ShieldCheck,Sparkles} from 'lucide-react'
import {Button,TextInput} from './Primitives'
import {FormField,Panel,Subheading} from './UI'
import {get,post} from '../api'

type Experiment={
 id:string;status:string;goal:string;paths:string[];reason?:string;
 baseCommit?:string;candidateCommit?:string;adoptedCommit?:string;
 validation?:{mode:string;baselinePassed:boolean;candidatePassed:boolean;
  improvedFailingBaseline:boolean;candidateExitCode?:number;candidateOutput?:string};
 usage?:{modelCalls:number;tokens:number};strategy?:string;
 createdAt?:string;jobId?:string
}
const working=new Set(['queued','isolating','baseline_testing','generating','candidate_testing'])
export function RsiPanel(){
 const [goal,setGoal]=useState(''),[paths,setPaths]=useState('server/model-router.mjs')
 const [test,setTest]=useState('node-tests')
 const [items,setItems]=useState<Experiment[]>([])
 const [busy,setBusy]=useState(''),[error,setError]=useState(''),[notice,setNotice]=useState('')
 const refresh=async()=>{try{setItems(await get<Experiment[]>('/api/rsi/experiments'))}catch(e){setError((e as Error).message)}}
 useEffect(()=>{
  void refresh()
  const interval=setInterval(()=>{void refresh()},5000)
  return()=>clearInterval(interval)
 },[])
 const request=async(url:string,data:unknown={})=>{
  setBusy(url);setError('');setNotice('')
  try{await post(url,data);await refresh();setNotice('요청을 등록했습니다.')}
  catch(e){setError((e as Error).message)}finally{setBusy('')}
 }
 const start=()=>{
  const list=paths.split(/[\n,]+/).map(x=>x.trim()).filter(Boolean)
  if(!goal.trim()||!list.length){setError('개선 목표와 수정 가능한 소스 경로가 필요합니다.');return}
  void request('/api/rsi/experiments',{goal:goal.trim(),paths:list,test})
 }
 return <Panel className="mb-5 min-w-0 p-5">
  <Subheading title="격리 RSI 실험실" extra={<Button variant="secondary" onClick={()=>void refresh()}><RefreshCw size={14}/> 새로고침</Button>}/>
  <p className="mb-4 text-sm text-[#6C6359]">별도 Git worktree에서 개선안을 생성하고 기준본과 같은 고정 테스트로 비교합니다. 통과한 변경도 승인하기 전에는 원본 소스에 적용되지 않습니다.</p>
  <div className="grid gap-3 md:grid-cols-2">
   <FormField label="개선 목표"><TextInput value={goal} onChange={e=>setGoal(e.target.value)} placeholder="예: 모델 라우팅 실패 복구 능력 개선"/></FormField>
   <FormField label="수정 허용 파일 (쉼표 구분)"><TextInput value={paths} onChange={e=>setPaths(e.target.value)} placeholder="server/model-router.mjs"/></FormField>
   <FormField label="동결 테스트 종류">
    <select className="w-full rounded-lg border border-[#DCD6CB] bg-white px-3 py-2 text-sm" value={test} onChange={e=>setTest(e.target.value)}>
     <option value="node-tests">Node 기본 테스트 전체</option>
     <option value="npm-test">npm test</option>
    </select>
   </FormField>
   <div className="flex items-end"><Button variant="primary" disabled={Boolean(busy)||!goal.trim()} onClick={start}><Sparkles size={15}/> RSI 실험 시작</Button></div>
  </div>
  {error&&<p className="mt-3 text-sm text-red-700">{error}</p>}
  {notice&&<p className="mt-3 text-sm text-green-700">{notice}</p>}
  <div className="mt-5 grid gap-3">
   {items.length===0&&<p className="text-sm text-[#8A8176]">실험 기록이 없습니다.</p>}
   {items.slice(0,12).map(row=><div key={row.id} className="rounded-xl border border-[#E7E0D5] bg-[#FCFAF6] p-4">
    <div className="flex flex-wrap items-center justify-between gap-2">
     <strong className="max-w-full break-words text-sm">{row.goal}</strong>
     <span className="rounded-md bg-[#EEE9DF] px-2 py-1 text-xs font-semibold">{row.status}</span>
    </div>
    <p className="mt-2 break-words text-xs text-[#6C6359]">{row.paths.join(', ')}</p>
    {row.reason&&<p className="mt-2 text-xs text-[#85523B]">{row.reason}</p>}
    {row.validation&&<p className="mt-2 text-xs">
     <ShieldCheck size={13} className="mr-1 inline"/> 기준 테스트 {row.validation.baselinePassed?'통과':'실패'} · 후보 테스트 {row.validation.candidatePassed?'통과':'실패'}
     {row.validation.improvedFailingBaseline?' · 기준 실패 → 후보 통과':''}
    </p>}
    {row.candidateCommit&&<p className="mt-1 font-mono text-[11px] text-[#8A8176]">후보 커밋 {row.candidateCommit.slice(0,12)}</p>}
    <div className="mt-3 flex flex-wrap gap-2">
     {row.status==='pending_approval'&&<Button variant="primary" disabled={Boolean(busy)} onClick={()=>{
      if(window.confirm('테스트 통과 후보를 원본 Git 소스에 fast-forward로 채택하시겠습니까?'))void request('/api/rsi/experiments/'+row.id+'/accept')
     }}><GitMerge size={14}/> 검증본 채택</Button>}
     {row.status==='accepted'&&<Button variant="secondary" disabled={Boolean(busy)} onClick={()=>void request('/api/rsi/experiments/'+row.id+'/next')}><Sparkles size={14}/> 후손 실험</Button>}
     {working.has(row.status)&&<Button variant="secondary" disabled={Boolean(busy)} onClick={()=>void request('/api/rsi/experiments/'+row.id+'/cancel')}>실험 중단</Button>}
    </div>
   </div>)}
  </div>
 </Panel>
}
