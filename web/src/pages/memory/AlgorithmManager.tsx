import {useState} from 'react'
import {Button,SelectInput} from '../../components/Primitives'
import {Notice,useRemote} from '../../components/UI'
import {get,post} from '../../api'
import {useLocale} from '../../i18n'
import {rows,scalar,observedRate,type EvidenceRow} from '../../utils/intelligenceView'
export function AlgorithmManager(){
 const en=useLocale()==='en',tr=(ko:string,eng:string)=>en?eng:ko
 const data=useRemote<{kinds?:EvidenceRow[]}>('/api/algorithms',{})
 const history=useRemote<{items?:EvidenceRow[]}>('/api/algorithms/evaluations',{})
 const [kind,setKind]=useState(''),[busy,setBusy]=useState(false),[error,setError]=useState(''),[notice,setNotice]=useState('')
 const [report,setReport]=useState<EvidenceRow|null>(null)
 const kinds=rows(data.value.kinds),group=kinds.find(k=>k.kind===kind)||kinds[0]
 const current=report||rows(history.value.items)[0]
 const perform=async(action:()=>Promise<void>)=>{
  if(busy)return
  setBusy(true);setError('');setNotice('')
  try{await action();await Promise.all([data.reload(),history.reload()]);setNotice(tr('서버 기록을 갱신했습니다.','Server records refreshed.'))}
  catch(e){setError((e as Error).message);await data.reload()}
  finally{setBusy(false)}
 }
 const change=(r:EvidenceRow,action:'activate'|'disable',force=false)=>{
  if(!r.id||r.retired||busy||data.error||data.loading)return
  const question=action==='disable'?tr('이 알고리즘을 비활성화하시겠습니까?','Disable this algorithm?'):force?tr('평가 추천 없이 수동 전환합니다. 실행 정책을 변경하시겠습니까?','Switch without a promotion recommendation?'):tr('서버 검증 조건에 따라 활성화하시겠습니까?','Activate under server evaluation rules?')
  if(window.confirm(question))void perform(async()=>{await post('/api/algorithms/'+encodeURIComponent(r.id)+'/'+action,action==='activate'?{force}:{});setReport(null)})
 }
 return <section className="grid gap-3 rounded-xl border border-[#E7E0D5] p-3">
  <div className="flex flex-wrap items-center justify-between gap-2"><strong className="text-sm">{tr('알고리즘 관리 및 비교','Algorithm management & comparison')}</strong><Button size="sm" disabled={busy} onClick={()=>void perform(async()=>{setReport(await get<EvidenceRow>('/api/algorithms/evaluate'))})}>{tr('실제 평가 실행','Run evaluation')}</Button></div>
  <p className="text-xs text-[#7C7469]">{tr('결정론적 평가와 실제 작업 성과는 별개입니다. 평가에는 모델 호출이 없습니다.','Fixture evaluations and observed outcomes are separate. Evaluation uses no model calls.')}</p>
  <Notice message={error||data.error||history.error}/>{notice&&<p role="status" className="text-xs">{notice}</p>}
  {data.loading?<p>{tr('조회 중','Loading')}</p>:group?<><label className="text-xs">{tr('알고리즘 종류','Algorithm category')}<SelectInput value={String(group.kind)} onChange={e=>setKind(e.target.value)}>{kinds.map(k=><option key={k.kind} value={k.kind}>{k.label||k.kind}</option>)}</SelectInput></label>
  <p className="rounded-lg bg-[#F3EEE5] p-2 text-xs">{tr('현재 활성화','Currently active')}: {scalar(group.active?.name||group.active?.implementation)}</p>
  {rows(group.algorithms).map(r=><article key={r.id} className="rounded-lg border border-[#EFE9DF] p-3 text-xs">
   <div className="flex flex-wrap justify-between gap-2"><strong>{scalar(r.name||r.implementation)}</strong><span>{r.retired?tr('지원 종료','Retired'):scalar(r.status)}</span></div><p className="mt-1 break-words text-[#7C7469]">{r.description||r.implementation}</p>
   <div className="mt-2 grid grid-cols-2 gap-2 sm:grid-cols-4">{[[tr('관측 성공률','Observed success'),observedRate(r.stats)],[tr('실패','Failures'),scalar(r.stats?.failures)],[tr('에이전트','Agents'),scalar(r.stats?.agents)],[tr('리비전','Revision'),scalar(r.revision)]].map(([label,value])=><div key={label}><small className="block text-[#8A8176]">{label}</small><strong className="break-all">{value}</strong></div>)}</div>
   {!r.retired&&<div className="mt-2 flex flex-wrap gap-2">{r.status!=='active'&&<><Button size="sm" disabled={busy||!!data.error} onClick={()=>change(r,'activate')}>{tr('검증 후 활성화','Checked activation')}</Button><Button size="sm" variant="ghost" disabled={busy||!!data.error} onClick={()=>change(r,'activate',true)}>{tr('수동 전환','Manual switch')}</Button></>}{r.status!=='disabled'&&<Button size="sm" variant="danger" disabled={busy||!!data.error} onClick={()=>change(r,'disable')}>{tr('비활성화','Disable')}</Button>}</div>}
   <details className="mt-2"><summary>{tr('파라미터','Parameters')}</summary><pre className="max-h-40 overflow-auto whitespace-pre-wrap break-all">{JSON.stringify(r.parameters??{},null,2)}</pre></details>
  </article>)}</>:<p>{tr('알고리즘 기록 없음','No algorithm records')}</p>}
  {current&&<section className="grid gap-2 border-t border-[#EFE9DF] pt-3"><strong>{tr('최근 평가 결과','Latest evaluation')}</strong><p className="text-xs">{scalar(current.generatedAt)} · {tr('항목','Dimensions')} {scalar(current.totalDimensions)}</p>
   {rows(current.dimensions).map(d=><div key={d.kind} className="rounded-lg border border-[#EFE9DF] p-2 text-xs"><strong>{scalar(d.kind)}</strong><p>{tr('현재','Active')}: {scalar(d.active)} ({scalar(d.activeValue)}) · {tr('최고','Winner')}: {scalar(d.winner)} ({scalar(d.winnerValue)}) · {scalar(d.recommendation)}</p>{rows(d.kernels).map(k=><div key={k.kernel} className="flex justify-between gap-2"><span>{scalar(k.kernel)}</span><strong>{scalar(k.value)}</strong></div>)}</div>)}
   <details className="text-xs"><summary>{tr('이전 평가 기록','Previous evaluations')}</summary>{rows(history.value.items).map(r=><details key={r.id}><summary>{scalar(r.generatedAt)}</summary><pre className="max-h-48 overflow-auto whitespace-pre-wrap break-all">{JSON.stringify(r.dimensions,null,2)}</pre></details>)}</details></section>}
 </section>
}
