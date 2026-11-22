export type TeamView='all'|'active'|'attention'|'finished'
type TeamCounts={total:number;active:number;attention:number;finished:number}
type WorkerNode={
 id:string;mode:'read'|'write';runtime:string;isolation:'shared'|'worktree';dependsOn:string[]
}

export function TeamStatusBar({counts,view,onChange,en}:{
 counts:TeamCounts;view:TeamView;onChange:(value:TeamView)=>void;en:boolean
}){
 const labels=[
  {id:'all' as const,ko:'전체 작업',english:'All teams',count:counts.total},
  {id:'active' as const,ko:'실행 중',english:'Active',count:counts.active},
  {id:'attention' as const,ko:'확인 필요',english:'Needs attention',count:counts.attention},
  {id:'finished' as const,ko:'종료',english:'Finished',count:counts.finished},
 ]
 return <div className="grid grid-cols-2 gap-2 md:grid-cols-4" role="group" aria-label={en?'Team run overview':'팀 실행 현황'}>
  {labels.map(filter=><button key={filter.id} type="button"
   aria-pressed={view===filter.id} onClick={()=>onChange(filter.id)}
   className={'min-w-0 rounded-xl border px-3 py-3 text-left transition-colors focus-visible:outline-2 focus-visible:outline-offset-2 '+
    (view===filter.id?'border-[#AA7355] bg-[#F8EEE8] shadow-sm':'border-divider bg-white hover:bg-[#FCF7F1]')}>
   <strong className="block text-[22px] leading-none tabular-nums text-[#342A24]">{filter.count}</strong>
   <span className="mt-2 block text-[11px] text-[#756B61]">{en?filter.english:filter.ko}</span>
  </button>)}
 </div>
}

export function TeamDependencyMap({steps,parallel,en}:{
 steps:WorkerNode[];parallel:number;en:boolean
}){
 const focusWorker=(id:string)=>{
  const node=document.getElementById('sub3-worker-'+id)
  node?.scrollIntoView({behavior:'smooth',block:'nearest'})
  node?.focus({preventScroll:true})
 }
 return <section className="mb-4 rounded-xl border border-[#DCE3DB] bg-[#F6F8F3] p-3" aria-label={en?'Worker dependency preview':'작업 의존성 미리보기'}>
  <div className="flex items-center justify-between gap-2">
   <strong className="text-[12px] text-[#2F4640]">{en?'Worker flow · Dependencies':'작업 흐름 · 의존성'}</strong>
   <span className="text-[10px] font-medium text-[#63776D]">{steps.length} Worker · {parallel} {en?'parallel':'동시 실행'}</span>
  </div>
  <p className="mt-1 text-[10px] leading-5 text-[#66736D]">{en?
   'Select a worker to edit below. Arrows show actual dependencies.':
   'Worker를 선택하면 아래 편집기로 이동합니다. 화살표는 실제 선행 작업 설정입니다.'}</p>
  <div className="mt-3 grid gap-2 sm:grid-cols-2 xl:grid-cols-3" role="list">
   {steps.map((step,index)=><div key={step.id} role="listitem" className="min-w-0">
    <button type="button" onClick={()=>focusWorker(step.id)}
     className="w-full min-w-0 rounded-lg border border-[#D9E2D8] bg-white p-3 text-left shadow-sm focus-visible:outline-2 focus-visible:outline-offset-2 hover:border-[#B68265]">
     <span className="flex items-center justify-between gap-2">
      <strong className="truncate text-[12px] text-[#33463D]">{index+1}. {step.id}</strong>
      <span className={'rounded px-1.5 py-0.5 text-[10px] '+(step.mode==='write'?'bg-[#F4E2D3] text-[#93563B]':'bg-[#E1EFE4] text-[#486851]')}>{step.mode==='write'?(en?'Write':'수정'):(en?'Read':'조사')}</span>
     </span>
     <span className="mt-1 block truncate text-[10px] text-[#7E8176]">{step.runtime} · {step.isolation==='worktree'?'worktree':'shared'}</span>
     <span className="mt-2 block break-words text-[10px] leading-5 text-[#576A61]">{step.dependsOn.length?
      step.dependsOn.join(' + ')+' → '+step.id:(en?'Independent · no dependencies':'독립 시작 · 선행 작업 없음')}</span>
    </button>
   </div>)}
  </div>
 </section>
}
