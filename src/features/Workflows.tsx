import {useState} from 'react'
import {ArrowDown,ArrowRight,ArrowUp,Check,Play,Plus,Settings2,Trash2,Workflow} from 'lucide-react'
import {del,post,type Agent} from '../api'
import {Alert,Badge,Button,Card,Dialog,Empty,Grid,Highlight,IconButton,Input,Page,PageHeader,Row,Section,Select,Stack,Text,Textarea} from '../design/Primitives'
import {useOperation,useResource} from '../design/useResource'
type Step={id?:string;name:string;prompt:string;agentId:string;providerId:string;model:string}
type Flow={id?:string;name:string;description:string;steps:Step[]}
const newStep=():Step=>({name:'새 단계',prompt:'',agentId:'general',providerId:'',model:''})
export function WorkflowsPage({openSession}:{openSession:(id:string)=>void}){
 const flows=useResource<Flow[]>('/api/workflows',[])
 const agents=useResource<Agent[]>('/api/agents',[])
 const jobs=useResource<any[]>('/api/jobs',[])
 const op=useOperation()
 const [editor,setEditor]=useState<Flow|null>(null)
 const [inputs,setInputs]=useState<Record<string,string>>({})
 const [notice,setNotice]=useState('')
 const fresh=()=>setEditor({name:'새 워크플로',description:'',steps:[newStep()]})
 const stepChange=(idx:number,part:Partial<Step>)=>{if(editor)setEditor({...editor,steps:editor.steps.map((s,i)=>i===idx?{...s,...part}:s)})}
 const move=(idx:number,dir:number)=>{if(!editor||idx+dir<0||idx+dir>=editor.steps.length)return;const steps=[...editor.steps];[steps[idx],steps[idx+dir]]=[steps[idx+dir],steps[idx]];setEditor({...editor,steps})}
 const save=()=>void op.run(async()=>{if(!editor?.name.trim()||editor.steps.some(s=>!s.prompt.trim()))throw Error('이름과 모든 단계 지시문이 필요합니다');await post('/api/workflows',editor);setEditor(null);setNotice('워크플로를 저장했습니다.');await flows.reload()})
 const start=(f:Flow)=>void op.run(async()=>{if(!f.id)return;await post('/api/workflows/'+f.id+'/run',{input:inputs[f.id]||''});setNotice(f.name+' 작업을 실행 대기열에 등록했습니다.');await jobs.reload()})
 const remove=(f:Flow)=>{if(f.id&&window.confirm(f.name+' 워크플로를 삭제할까요?'))void op.run(async()=>{await del('/api/workflows/'+f.id);await flows.reload()})}
 return <Page><PageHeader eyebrow="WORKFLOW ENGINE" title="워크플로" description="에이전트별 작업을 순서대로 실행하고 단계 결과를 다음 단계에 전달합니다." actions={<Button variant="primary" onClick={fresh}><Plus size={16}/> 워크플로 만들기</Button>}/>
 <Stack gap="lg">{op.error&&<Alert message={op.error} onClose={()=>op.setError('')}/>}
 {notice&&<Alert tone="positive" message={notice} onClose={()=>setNotice('')}/>}
 <Highlight title="단계별 실행" icon={<Workflow size={16}/>}>지시문에서 {'{{input}}'}은 시작 입력, {'{{previous}}'}는 이전 단계 결과입니다. 실패 시 실제 실행 상태가 기록됩니다.</Highlight>
 <Grid cols={2}>{flows.value.map(f=>{
  const job=jobs.value.find(j=>j.kind==='workflow'&&j.input?.workflowId===f.id)
  return <Card key={f.id}><Stack gap="md"><Row justify="between"><Row gap="sm"><span className="grid size-11 place-items-center rounded-xl bg-brand-soft text-brand"><Workflow size={20}/></span><Stack gap="xs"><Text size="lg" weight="bold">{f.name}</Text><Text size="xs" tone="subtle">{f.description||'순차 작업 흐름'}</Text></Stack></Row><Row gap="xs"><IconButton label="워크플로 편집" icon={<Settings2 size={17}/>} onClick={()=>setEditor({...f})}/><IconButton label="워크플로 삭제" icon={<Trash2 size={16}/>} onClick={()=>remove(f)}/></Row></Row><Stack gap="sm">{f.steps.map((step,i)=><Row key={step.id||i} gap="sm" wrap><Badge tone="brand">{i+1}</Badge><Text weight="semibold">{step.name}</Text><Text tone="subtle" size="xs">{agents.value.find(a=>a.id===step.agentId)?.name||step.agentId}</Text>{i<f.steps.length-1&&<ArrowRight size={13} className="text-subtle"/>}</Row>)}</Stack><Row gap="sm"><Input aria-label={f.name+' 실행 입력'} placeholder="실행 입력 · 선택사항" value={inputs[f.id||'']||''} onChange={e=>setInputs(x=>({...x,[f.id||'']:e.target.value}))}/><Button variant="primary" onClick={()=>start(f)} disabled={op.working}><Play size={16}/> 실행</Button></Row><Row justify="between" wrap><Text tone="subtle" size="xs">{f.steps.length}단계</Text>{job&&<Badge tone={job.status==='completed'?'positive':job.status==='failed'?'negative':'neutral'}>{job.status} · {job.stepIndex||0}/{job.stepTotal||f.steps.length}</Badge>}{job?.result?.sessionId&&<Button variant="ghost" size="sm" onClick={()=>openSession(job.result.sessionId)}>결과 보기 <ArrowRight size={14}/></Button>}</Row></Stack></Card>
 })}</Grid>{!flows.value.length&&<Empty icon={<Workflow size={22}/>} title="워크플로가 없습니다" description="작업 지시문과 실행 순서를 설정하세요." action={<Button variant="primary" onClick={fresh}>첫 워크플로 만들기</Button>}/>}
 </Stack>
 <Dialog open={Boolean(editor)} onClose={()=>setEditor(null)} title="워크플로 편집" description="각 단계는 지정된 에이전트의 모델을 사용합니다." wide footer={<><Button onClick={()=>setEditor(null)}>취소</Button><Button variant="primary" disabled={!editor?.name.trim()||editor.steps.some(x=>!x.prompt.trim())||op.working} onClick={save}><Check size={15}/> 저장</Button></>}>
 {editor&&<Stack gap="lg"><Input label="이름" value={editor.name} onChange={e=>setEditor({...editor,name:e.target.value})}/><Input label="설명" value={editor.description} onChange={e=>setEditor({...editor,description:e.target.value})}/><Section title={'단계 구성 · '+editor.steps.length+'개'}><Stack gap="sm">{editor.steps.map((step,i)=><Card key={step.id||i} pad="sm" tone="subtle"><Stack gap="sm"><Row justify="between"><Text weight="bold">{i+1}. {step.name}</Text><Row gap="xs"><IconButton size="sm" label="위로 이동" disabled={i===0} icon={<ArrowUp size={15}/>} onClick={()=>move(i,-1)}/><IconButton size="sm" label="아래로 이동" disabled={i===editor.steps.length-1} icon={<ArrowDown size={15}/>} onClick={()=>move(i,1)}/><IconButton size="sm" label="단계 삭제" disabled={editor.steps.length===1} icon={<Trash2 size={15}/>} onClick={()=>setEditor({...editor,steps:editor.steps.filter((_,j)=>i!==j)})}/></Row></Row><Input label="단계 이름" value={step.name} onChange={e=>stepChange(i,{name:e.target.value})}/><Select label="에이전트" value={step.agentId} onChange={e=>stepChange(i,{agentId:e.target.value})} items={agents.value.map(a=>({value:a.id,label:a.name}))}/><Textarea label="실행 지시문" rows={3} value={step.prompt} onChange={e=>stepChange(i,{prompt:e.target.value})} placeholder={'이전 단계의 결과: {{previous}}'}/></Stack></Card>)}</Stack></Section><Button disabled={editor.steps.length>=30} onClick={()=>setEditor({...editor,steps:[...editor.steps,newStep()]})}><Plus size={15}/> 단계 추가</Button></Stack>}
 </Dialog></Page>
}
