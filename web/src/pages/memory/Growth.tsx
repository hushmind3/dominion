import {t, useLocale, formatDateTime} from '../../i18n'
import {statusLabel} from '../../components/StatusPill'
import {rows,strings,scalar} from '../../utils/intelligenceView'
import {useRemote,Notice,Panel,Subheading} from '../../components/UI'

type Growth={
 runs?:number;ratedRuns?:number;succeeded?:number;failed?:number;successRate?:number;
 failureClasses?:Record<string,number>;toolFailures?:Record<string,number>;
 guidance?:{text?:string;confidence?:number;evidenceCount?:number}[];
}
type AgentRow={id?:string;name?:string;growth?:Growth}
type Synergy={agents?:AgentRow[];sharedMethods?:any[];algorithmStats?:any[]}
type UsageGroup={providerId?:string;model?:string;purpose?:string;calls?:number;totalTokens?:number;estimatedCalls?:number}
type Usage={calls?:number;totalTokens?:number;completionTokens?:number;groups?:UsageGroup[]}
type Transfer={
 principles?:any[];rules?:any[];usages?:any[];learningCases?:any[];
}
const number=(value:any)=>typeof value==='number'?value:0
const percent=(value:any)=>`${Math.round(number(value)*100)}%`
const label=(value:any,fallback=t('growth.none'))=>typeof value==='string'&&value?value:fallback

export function GrowthDashboard(){
 const en=useLocale()==='en'
 const synergy=useRemote<Synergy>('/api/synergy',{agents:[],sharedMethods:[],algorithmStats:[]})
 const usage=useRemote<Usage>('/api/model-usage',{groups:[]})
 const transfer=useRemote<Transfer>('/api/transfer',{principles:[],rules:[],usages:[],learningCases:[]})
 const agents=(synergy.value.agents||[]).filter(agent=>number(agent.growth?.runs)>0).sort((a,b)=>number(b.growth?.runs)-number(a.growth?.runs))
 const rated=agents.reduce((sum,agent)=>sum+number(agent.growth?.ratedRuns),0)
 const principles=transfer.value.principles||[]
 const usages=transfer.value.usages||[]
 const rules=transfer.value.rules||[]
 const activePrinciples=principles.filter(item=>item.status==='active').length
 const calls=number(usage.value.calls)
 const tokens=number(usage.value.totalTokens)
 const groups=(usage.value.groups||[]).slice(0,10)
 return <div className="mt-3 grid gap-4">
  <Notice message={synergy.error||usage.error||transfer.error}/>
  <div className="grid grid-cols-2 gap-2 lg:grid-cols-4">
   <div className="rounded-xl border border-[#EFE9DF] bg-white p-3"><small className="text-[10px] font-semibold text-[#8A8176]">{t('growth.verifiedRuns')}</small><strong className="block text-lg">{rated}</strong></div>
   <div className="rounded-xl border border-[#EFE9DF] bg-white p-3"><small className="text-[10px] font-semibold text-[#8A8176]">{t('growth.activePrinciples')}</small><strong className="block text-lg">{activePrinciples}</strong></div>
   <div className="rounded-xl border border-[#EFE9DF] bg-white p-3"><small className="text-[10px] font-semibold text-[#8A8176]">{t('growth.sharedRules')}</small><strong className="block text-lg">{rules.length}</strong></div>
   <div className="rounded-xl border border-[#EFE9DF] bg-white p-3"><small className="text-[10px] font-semibold text-[#8A8176]">{t('growth.modelCalls')}</small><strong className="block text-lg">{calls}</strong><small className="text-[10px] text-[#9C9388]">{tokens.toLocaleString()} {t('growth.tokens')}</small></div>
  </div>

  <Panel className="min-w-0 p-4">
   <Subheading title={t('growth.observedGrowth')} extra={<span className="text-[10px] text-[#9C9388]">{t('ui.label.verifiedRuns')}</span>}/>
   {!agents.length?<p className="text-sm text-[#7C7469]">{t('growth.noRuns')}</p>:<div className="grid gap-2">{agents.slice(0,10).map(agent=>{
    const growth=agent.growth||{}
    const failures=Object.entries(growth.toolFailures||{}).sort((a,b)=>number(b[1])-number(a[1])).slice(0,3)
    const guidance=(growth.guidance||[])[0]
    return <div className="rounded-lg border border-[#EFE9DF] p-3" key={agent.id}>
     <div className="flex flex-wrap items-center gap-2"><strong className="text-sm">{label(agent.name,agent.id)}</strong><span className="text-[11px] text-[#7C7469]">{t('growth.runMeta',{runs:number(growth.runs),rated:number(growth.ratedRuns)})}</span><span className="ml-auto rounded bg-[#F7E8E1] px-1.5 py-0.5 text-[10px] font-semibold text-[#A94E30]">{percent(growth.successRate)}</span></div>
     {failures.length>0&&<p className="mt-1 text-[11px] text-[#8A8176]">{t('growth.failedTools',{tools:failures.map(([tool,count])=>`${tool} ${number(count)}`).join(' · ')})}</p>}
     {guidance?.text&&<p className="mt-1 text-[11px] text-[#5C544A]">{t('growth.guidance',{text:guidance.text})}</p>}
    </div>
   })}</div>}
  </Panel>

  <Panel className="min-w-0 p-4">
   <Subheading title={t('growth.modelUsage')} extra={<span className="text-[10px] text-[#9C9388]">{t('ui.label.apiLocal')}</span>}/>
   {!groups.length?<p className="text-sm text-[#7C7469]">{t('growth.noCalls')}</p>:<div className="grid gap-1 text-[11px]">{groups.map((group,index)=><div className="grid grid-cols-[1fr_auto_auto] gap-2 rounded-lg border border-[#EFE9DF] px-3 py-2" key={index}><span className="break-all font-semibold text-[#4A443C]">{label(group.model,t('growth.modelUnknown'))}</span><span className="text-[#7C7469]">{label(group.purpose,t('growth.purposeOther'))} · {t('growth.calls',{count:number(group.calls)})}</span><span className="text-[#7C7469]">{number(group.totalTokens).toLocaleString()} {t('growth.tokens')}</span></div>)}</div>}
  </Panel>

  <Panel className="min-w-0 p-4">
   <Subheading title={t('growth.transfer')} extra={<span className="text-[10px] text-[#9C9388]">{t('ui.label.provenance')}</span>}/>
   {!principles.length&&!rules.length?<p className="text-sm text-[#7C7469]">{t('growth.noPrinciples')}</p>:<div className="grid gap-2">
    {principles.slice(0,6).map(principle=><div className="rounded-lg border border-[#EFE9DF] p-3" key={principle.id}>
     <div className="flex flex-wrap gap-2 text-[11px]"><strong>{label(principle.kind,t('growth.principle'))}</strong><span className="text-[#7C7469]">{label(principle.status)} · {t('growth.confidence',{value:percent(principle.confidence)})}</span><span className="ml-auto rounded bg-[#F7E8E1] px-1.5 py-0.5 text-[10px] text-[#A94E30]">{t('growth.cases',{count:number((principle.supportingCases||[]).length)})}</span></div>
     <p className="mt-1 text-[12px] text-[#4A443C]">{label(principle.principle)}</p>
     <p className="mt-1 text-[10px] text-[#8A8176]">{t('growth.contributors',{models:(principle.supportingModels||[]).join(', ')||t('growth.none')})} · {t('growth.agentsCount',{count:(principle.supportingAgents||[]).length})} · {t('growth.counterexamples',{count:number((principle.counterexamples||[]).length)})}</p>
    </div>)}
    {rules.slice(0,6).map(rule=><div className="rounded-lg border border-[#EFE9DF] p-3" key={rule.id}>
     <div className="flex flex-wrap gap-2 text-[11px]"><strong>{t('growth.conditional')}</strong><span className="text-[#7C7469]">{label(rule.status)} · {t('growth.confidence',{value:percent(rule.confidence)})}</span><span className="ml-auto text-[10px] text-[#A94E30]">{t('growth.cases',{count:number((rule.supportingCases||[]).length)})}</span></div>
     <p className="mt-1 text-[12px] text-[#4A443C]">{label(rule.trigger)} → {label(rule.intervention?.tool,t('growth.tool'))}</p>
     <p className="mt-1 text-[10px] text-[#8A8176]">{t('growth.agentsCount',{count:(rule.supportingAgents||[]).length})} · {t('growth.counterexamples',{count:number((rule.counterexamples||[]).length)})}</p>
    </div>)}
   </div>}
  </Panel>

  <Panel className="min-w-0 p-4">
   <Subheading title={t('growth.usageLog')} extra={<span className="text-[10px] text-[#9C9388]">{usages.length}</span>}/>
   {!usages.length?<p className="text-sm text-[#7C7469]">{t('growth.none')}</p>:<div className="grid gap-1">{usages.slice(0,8).map((usage:any)=><div className="rounded-lg border border-[#EFE9DF] px-3 py-2 text-[11px]" key={usage.id}>
    <div className="flex flex-wrap items-center gap-2"><strong className="min-w-0 flex-1 truncate">{label(usage.principle,t('growth.principle'))}</strong><span className="rounded bg-[#F7E8E1] px-1.5 py-0.5 text-[10px] text-[#A94E30]">{statusLabel(usage.outcome||usage.status)||t('growth.none')}</span></div>
    <p className="mt-1 text-[10px] text-[#8A8176]">{t('growth.usageReused',{model:label(usage.model,t('growth.modelUnknown'))})} · {t('growth.usageContributed',{models:(usage.contributingModels||[]).join(', ')||t('growth.none')})} · {t('growth.usageAgent',{agent:label(usage.agentId,t('growth.none'))})}</p>
    <p className="text-[10px] text-[#9C9388]">{formatDateTime(usage.execution?.startedAt||usage.createdAt)}</p>
    <div className="mt-2 flex flex-wrap items-center gap-2"><span className="rounded bg-[#F3EEE5] p-1">{strings(usage.contributingAgents).join(', ')||'—'}</span><span>→</span><span className="rounded bg-[#F3EEE5] p-1">{scalar(usage.agentId)}</span><span>{scalar(usage.execution?.taskOutcome)}</span></div>
   </div>)}</div>}
  </Panel>

  <Panel className="min-w-0 p-4"><Subheading title={en?'Learning case evidence':'학습 사례 및 근거'}/>{rows(transfer.value.learningCases).length?rows(transfer.value.learningCases).slice(0,20).map((c,i)=><details key={c.id||i} className="border-b border-[#EFE9DF] p-2 text-xs"><summary>{scalar(c.id)} · {scalar(c.status||c.outcome)}</summary><pre className="max-h-48 overflow-auto whitespace-pre-wrap break-all">{JSON.stringify(c,null,2)}</pre></details>):<p className="text-xs">—</p>}</Panel>
 </div>
}
