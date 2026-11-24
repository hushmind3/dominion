export type EvidenceRow=Record<string,any>
export const rows=(v:unknown):EvidenceRow[]=>Array.isArray(v)?v.filter(x=>x&&typeof x==='object'&&!Array.isArray(x)):[]
export const obj=(v:unknown):EvidenceRow=>v&&typeof v==='object'&&!Array.isArray(v)?v as EvidenceRow:{}
export const scalar=(v:unknown):string=>typeof v==='string'||typeof v==='number'||typeof v==='boolean'?String(v):'—'
export const strings=(v:unknown):string[]=>Array.isArray(v)?v.filter((x):x is string=>typeof x==='string'):[]
export function observedRate(stats:unknown):string{
 const row=obj(stats)
 return typeof row.uses==='number'&&row.uses>0&&typeof row.successes==='number'?Math.round(row.successes/row.uses*100)+'% ('+row.successes+'/'+row.uses+')':'—'
}
export function sdkSteeringAllowed(run:{status?:string;sessionId?:string;readOnly?:boolean;steeringGate?:{closed?:boolean};steering?:unknown[]}){
 return Boolean(run.sessionId)&&!run.readOnly&&['queued','running'].includes(run.status||'')&&run.steeringGate?.closed!==true&&(run.steering?.length||0)<8
}
export const steeringBytes=(text:string)=>new TextEncoder().encode(text.trim()).length

/** Layout only: preserve recorded dependency edges and never infer execution state. */
export function dependencyLevels<T extends {id:string;dependsOn?:string[]}>(steps:T[]):{levels:T[][];unresolved:T[]}{
 const levels:T[][]=[],known=new Set<string>(),remaining=[...steps]
 while(remaining.length){
  const ready=remaining.filter(s=>(s.dependsOn||[]).every(dep=>known.has(dep)))
  if(!ready.length)break
  levels.push(ready)
  for(const s of ready){known.add(s.id);remaining.splice(remaining.indexOf(s),1)}
 }
 return {levels,unresolved:remaining}
}
