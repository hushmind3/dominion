import {Bot,BrainCircuit,ChevronRight,Command,FileDown,History,Layers3,ListTree,MessageSquare,Play,Search,Settings2,Shield,Square,Terminal,Workflow} from 'lucide-react'
import {Button} from './Primitives'

export type SlashChoice={
 key:string;command:string;name:string;description:string;group:string;
 requiresArg?:boolean;submenu?:boolean;hint?:string
}
type Selectable={id:string;name:string}
type Model={id:string;name:string}
const defs:SlashChoice[]=[
 {key:'new',command:'/new',name:'새 대화',description:'새 세션을 시작합니다',group:'세션'},
 {key:'history',command:'/history',name:'대화 기록',description:'저장된 대화 목록을 표시합니다',group:'세션'},
 {key:'name',command:'/name',name:'세션 이름 변경',description:'현재 세션의 이름을 변경합니다',group:'세션',requiresArg:true,hint:'새 이름'},
 {key:'clear',command:'/clear',name:'대화 초기화',description:'현재 대화의 메시지를 지우고 처음부터 시작합니다',group:'세션'},
 {key:'fork',command:'/fork',name:'대화 분기',description:'현재 대화 기록을 복제한 새 세션을 만듭니다',group:'세션'},
 {key:'export',command:'/export',name:'기록 내보내기',description:'현재 대화를 JSON 파일로 저장합니다',group:'세션'},
 {key:'compact',command:'/compact',name:'컨텍스트 압축',description:'모델로 앞선 대화를 요약하고 최근 메시지를 유지합니다',group:'세션'},
 {key:'goal',command:'/goal',name:'자율 작업 실행',description:'실행 엔진에 목표 작업을 등록합니다',group:'실행',requiresArg:true,hint:'실행할 내용'},
 {key:'stop',command:'/stop',name:'실행 중지',description:'현재 세션에서 진행 중인 실행을 취소합니다',group:'실행'},
 {key:'status',command:'/status',name:'실행 상태',description:'연결 및 실행 현황을 확인합니다',group:'실행'},
 {key:'think',command:'/think',name:'추론 표시',description:'추론 표시를 켜거나 끕니다 (on / off)',group:'모델 · 에이전트'},
 {key:'model',command:'/model',name:'모델 전환',description:'현재 제공자에서 사용할 모델을 선택합니다',group:'모델 · 에이전트',submenu:true},
 {key:'agent',command:'/agent',name:'에이전트 전환',description:'작업을 실행할 에이전트를 선택합니다',group:'모델 · 에이전트',submenu:true},
 {key:'tools',command:'/tools',name:'도구 목록',description:'등록된 기본 도구 및 MCP 도구를 조회합니다',group:'도구 · 확장'},
 {key:'mcp',command:'/mcp',name:'MCP 설정',description:'MCP 연결과 서버 설정으로 이동합니다',group:'도구 · 확장'},
 {key:'skills',command:'/skills',name:'스킬 관리',description:'설치된 스킬과 확장 기능을 관리합니다',group:'도구 · 확장'},
 {key:'memory',command:'/memory',name:'기억 관리',description:'저장된 장기기억을 엽니다',group:'도구 · 확장'},
 {key:'workflows',command:'/workflows',name:'워크플로',description:'실행 엔진과 워크플로 화면을 엽니다',group:'화면'},
 {key:'agents',command:'/agents',name:'에이전트 관리',description:'에이전트 설정 화면을 엽니다',group:'화면'},
 {key:'settings',command:'/settings',name:'설정',description:'플랫폼 설정 화면을 엽니다',group:'화면'},
 {key:'help',command:'/help',name:'명령어 도움말',description:'사용 가능한 명령어와 사용법을 확인합니다',group:'화면'}
]
export const slashCommands=defs
export const normalizeSlashName=(text:string)=>text.trim().split(/\s+/)[0]?.slice(1).toLowerCase()||''
export function slashCandidates(text:string,agents:Selectable[],models:Model[]):SlashChoice[]{
 if(!text.startsWith('/')||text.includes('\n'))return [];
 if(/^\/model\s/i.test(text)){
  const query=text.replace(/^\/model\s+/i,'').trim().toLocaleLowerCase()
  return models.filter(m=>m.id.toLocaleLowerCase().includes(query)||m.name.toLocaleLowerCase().includes(query))
   .slice(0,30).map(m=>({key:'model:'+m.id,command:'/model '+m.id,name:m.name,description:m.id,group:'모델 선택'}))
 }
 if(/^\/agent\s/i.test(text)){
  const query=text.replace(/^\/agent\s+/i,'').trim().toLocaleLowerCase()
  return agents.filter(a=>a.id.toLocaleLowerCase().includes(query)||a.name.toLocaleLowerCase().includes(query))
   .slice(0,30).map(a=>({key:'agent:'+a.id,command:'/agent '+a.id,name:a.name,description:a.id,group:'에이전트 선택'}))
 }
 if(/\s/.test(text))return [];
 const query=text.slice(1).toLocaleLowerCase()
 return defs.filter(item=>[item.key,item.name,item.description].some(s=>s.toLocaleLowerCase().includes(query)))
}
const icons:Record<string,typeof Command>={
 new:MessageSquare,history:History,name:MessageSquare,clear:Square,fork:Layers3,export:FileDown,
 compact:ListTree,goal:Play,stop:Square,status:Search,think:BrainCircuit,model:BrainCircuit,
 agent:Bot,tools:Terminal,mcp:Terminal,skills:Layers3,memory:History,
 workflows:Workflow,agents:Bot,settings:Settings2,help:Shield
}
export function SlashMenu({items,active,onChoose}:{items:SlashChoice[];active:number;onChoose:(item:SlashChoice)=>void}){
 const groups=[...new Set(items.map(item=>item.group))]
 return <div role="listbox" aria-label="슬래시 명령어" id="dominion-slash-menu"
  className="absolute bottom-full left-0 right-0 z-30 mb-3 max-h-[min(400px,60vh)] overflow-y-auto rounded-2xl border border-slate-200 bg-white p-2 shadow-[0_16px_60px_rgba(15,23,42,.2)]">
  <div className="flex items-center gap-2 border-b border-slate-100 px-3 pb-2 pt-1 text-xs text-slate-500">
   <Command size={15}/> 명령어 검색
   <span className="ml-auto text-[10px] text-slate-400">↑↓ 이동 · Enter 선택 · Esc 닫기</span>
  </div>
  {items.length===0&&<p className="px-3 py-6 text-center text-xs text-slate-500">해당 명령어나 선택 항목이 없습니다.</p>}
  {groups.map(group=><div key={group}>
   <p className="px-3 pb-1 pt-3 text-[10px] font-bold tracking-[.12em] text-slate-400">{group}</p>
   {items.map((item,index)=>{
    if(item.group!==group)return null
    const Icon=icons[item.key]|| (item.key.startsWith('model:')?BrainCircuit:Bot)
    return <Button key={item.key} type="button" variant="ghost" role="option" aria-selected={active===index}
      onMouseDown={e=>e.preventDefault()} onClick={()=>onChoose(item)}
      className={'!flex !w-full !items-center !justify-start !gap-3 !rounded-lg !border-0 !px-3 !py-2 !text-left '+(active===index?'!bg-blue-50 !text-blue-700':'!text-slate-700')}>
      <span className={'grid size-8 shrink-0 place-items-center rounded-lg '+(active===index?'bg-blue-100':'bg-slate-100')}><Icon size={16}/></span>
      <span className="min-w-0 flex-1"><strong className="block truncate text-sm font-semibold">
       {item.key.startsWith('model:')||item.key.startsWith('agent:')?item.name:item.command}
      </strong><small className="block truncate text-[11px] font-normal text-slate-500">{item.key.includes(':')?item.description:item.name+' · '+item.description}</small></span>
      {item.submenu?<ChevronRight size={14} className="shrink-0 text-slate-400"/>:
       item.requiresArg?<span className="shrink-0 text-[10px] text-slate-400">{item.hint}…</span>:null}
    </Button>
   })}
  </div>)}
 </div>
}
