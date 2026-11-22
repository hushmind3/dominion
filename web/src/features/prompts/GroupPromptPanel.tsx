import {useEffect,useState} from 'react'
import {Check,LoaderCircle,Save,UsersRound} from 'lucide-react'
import {post,type Agent} from '../../api'
import {useRemote} from '../../hooks/useRemote'
import {useLocale} from '../../i18n'
import {Button,SelectInput,TextArea} from '../../components/Primitives'
import {FormField,Notice} from '../../components/UI'
import {EmptyState,FormActions} from '../../components/ui/WorkspaceUI'
import {PromptSection,PromptConflict} from './PromptSection'
import {usePromptDraft} from './usePromptDraft'

type Room={id:string;name:string;agentIds:string[]}
type Preview={common:string;prompt:string;commonOverride?:string|null;finalOverride?:string|null}

/** Group prompt fields have one editor, inside the prompt workspace. */
export function GroupPromptPanel({initialRoomId}:{initialRoomId?:string}){
 const en=useLocale()==='en';const tr=(ko:string,english:string)=>en?english:ko
 const rooms=useRemote<Room[]>('/api/rooms',[]),agents=useRemote<Agent[]>('/api/agents',[])
 const [selection,setSelection]=useState(initialRoomId||''),[agentSelection,setAgentSelection]=useState('')
 useEffect(()=>{if(initialRoomId)setSelection(initialRoomId)},[initialRoomId])
 const room=rooms.value.find(room=>room.id===selection)||rooms.value[0]
 const roster=agents.value.filter(agent=>room?.agentIds.includes(agent.id))
 const agent=roster.find(agent=>agent.id===agentSelection)||roster[0]
 const preview=useRemote<Preview|null>(room?'/api/rooms/'+encodeURIComponent(room.id)+'/system-prompt?agentId='+encodeURIComponent(agent?.id||''):null,null)
 const ready=Boolean(preview.value)&&!preview.loading&&!preview.error
 const common=usePromptDraft('prompt:room:common:'+room?.id,preview.value?.commonOverride??preview.value?.common??'',ready)
 const full=usePromptDraft('prompt:room:full:'+room?.id,preview.value?.finalOverride??preview.value?.prompt??'',ready)
 const [busy,setBusy]=useState(false),[error,setError]=useState(''),[notice,setNotice]=useState('')
 useEffect(()=>{setError('');setNotice('')},[room?.id])
 const dirty=common.dirty||full.dirty,conflict=common.conflict||full.conflict
 const save=async()=>{
  if(busy||!room||!preview.value||!dirty||conflict)return
  setBusy(true);setError('');setNotice('')
  try{
   await post('/api/rooms/'+encodeURIComponent(room.id)+'/system-prompt',{
    ...(common.dirty?{common:common.value,expectedCommon:preview.value.commonOverride??null}:{}),
    ...(full.dirty?{final:full.value,expectedFinal:preview.value.finalOverride??null}:{}),
   })
   common.accept(common.value);full.accept(full.value);await preview.reload()
   setNotice(tr('그룹 지침을 저장했습니다. 다음 답변부터 적용됩니다.','Group instructions saved for subsequent turns.'))
  }catch(caught){setError((caught as Error).message);await preview.reload()}
  finally{setBusy(false)}
 }
 if(rooms.loading)return <p role="status" className="flex items-center gap-2 text-sm text-muted"><LoaderCircle size={18} className="animate-spin"/>{tr('그룹 불러오는 중','Loading groups')}</p>
 if(rooms.error&&!rooms.value.length)return <Notice message={rooms.error} onRetry={()=>void rooms.reload()}/>
 if(!rooms.value.length)return <EmptyState icon={UsersRound} title={tr('아직 만든 그룹이 없습니다','No groups yet')} description={tr('그룹을 만들면 이곳에서 그룹별 지침을 편집할 수 있습니다.','Create a group to edit its instructions here.')} action={<a href="#/rooms" className="rounded-xl bg-brand px-4 py-3 text-sm font-semibold text-white">{tr('그룹 만들기','Create a group')}</a>}/>
 return <div className="grid gap-5">
  <FormField label={tr('수정할 그룹','Group to edit')}><SelectInput aria-label={tr('수정할 그룹','Group to edit')} disabled={busy} value={room?.id||''} onChange={event=>{setSelection(event.target.value);setAgentSelection('')}}>{rooms.value.map(room=><option key={room.id} value={room.id}>{room.name}</option>)}</SelectInput></FormField>
  <Notice message={error||rooms.error||preview.error} onRetry={()=>{void rooms.reload();void preview.reload()}}/>
  {notice&&<p role="status" className="flex items-center gap-2 text-sm text-success"><Check size={16}/>{notice}</p>}
  <PromptSection id="room-prompt-common" title={tr('그룹 공통 지침','Shared group instructions')} scope={room?.name}>
   <p>{tr('이 그룹의 모든 참여자에게 적용할 답변 규칙을 적어 주세요.','Set the response rules for all participants in this group.')}</p>
   <TextArea rows={8} aria-label={tr('그룹 공통 지침 내용','Group instruction text')} disabled={!ready||busy} value={common.value} onChange={event=>common.change(event.target.value)}/>
   {common.conflict&&<PromptConflict current={common.confirmed} onContinue={common.rebase} onDiscard={common.discard} english={en}/>}
  </PromptSection>
  <PromptSection id="room-prompt-final" title={tr('그룹 시스템 프롬프트','Group system prompt')} scope={room?.name}>
   <p>{tr('자동으로 합쳐진 지침입니다. 직접 고쳐 저장하면 이 그룹의 공통 지침과 페르소나를 합치는 방식을 대체합니다.','This is the composed system text. Saving changes replaces automatic composition of group instructions and personas.')}</p>
   {roster.length>1&&<FormField label={tr('미리볼 AI','Preview for AI')}><SelectInput aria-label={tr('미리볼 AI','Preview for AI')} value={agent?.id||''} disabled={busy} onChange={event=>setAgentSelection(event.target.value)}>{roster.map(agent=><option key={agent.id} value={agent.id}>{agent.name}</option>)}</SelectInput></FormField>}
   <TextArea rows={12} aria-label={tr('그룹 시스템 프롬프트 내용','Group system prompt text')} className="font-mono !text-sm !leading-7" disabled={!ready||busy} value={full.value} onChange={event=>full.change(event.target.value)}/>
   {full.conflict&&<PromptConflict current={full.confirmed} onContinue={full.rebase} onDiscard={full.discard} english={en}/>}
   <p>{tr('AI별 성격과 역할은 AI·페르소나 탭에서 관리합니다.','Edit each AI’s personality and role in the AIs & personas tab.')}</p>
  </PromptSection>
  <FormActions description={dirty?tr('저장하지 않은 변경 사항','Unsaved changes'):undefined}>
   <Button variant="ghost" disabled={!dirty||busy} onClick={()=>{common.discard();full.discard()}}>{tr('변경 취소','Discard changes')}</Button>
   <Button variant="primary" disabled={!ready||!dirty||conflict||busy||common.value.length>32000||full.value.length>32000} onClick={()=>void save()}><Save size={16}/>{tr('그룹 지침 저장','Save group instructions')}</Button>
  </FormActions>
 </div>
}
