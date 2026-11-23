/** Sub2 uses execution.providerRounds; Sub4 stored a different call shape.
 * Keep projections, provider receipts and unavailable figures distinct. */
export type NumberLike=number|null|undefined
export type ProviderRound={
 round?:number;completedCallIndex?:number;promptTokens?:NumberLike;
 completionTokens?:NumberLike;outputTokens?:NumberLike;totalTokens?:NumberLike;
 cachedPromptTokens?:NumberLike;uncachedPromptTokens?:NumberLike;
 estimatedPromptTokens?:NumberLike;estimatedOutputTokens?:NumberLike;
 nativeInputTokensEstimated?:NumberLike;usageExact?:boolean;
 toolsAdvertised?:NumberLike;providerId?:string;model?:string;
}
export type Footprint={totalEstimatedTokens?:NumberLike;toolDefinitions?:NumberLike;
 components?:Record<string,number|null|undefined>}
export type ExecutionInputContext={
 policy?:string;systemTokens?:NumberLike;historyTokens?:NumberLike;
 retrievalTokens?:NumberLike;memoryTokens?:NumberLike;
 toolSchemaTokens?:NumberLike;reservedOutputTokens?:NumberLike;
 totalInputTokens?:NumberLike;providerPromptTokens?:NumberLike;
 providerCachedPromptTokens?:NumberLike;providerNewPromptTokens?:NumberLike;
 providerOutputTokens?:NumberLike;
 preAdapterWireProfile?:Footprint;wireRounds?:{breakdown?:Footprint}[];
 modelCallUsage?:ProviderRound[];
}
export type ExecutionEvidence={
 id:string;status?:string;contextPolicy?:string;
 modelCalls?:NumberLike;toolCalls?:NumberLike;tokens?:NumberLike;
 discoveredTools?:string[];inputContext?:ExecutionInputContext;
 providerRounds?:ProviderRound[];taskOutcome?:string|null;
 verified?:boolean;acceptancePassed?:boolean;
 failureClass?:string|null;reason?:string|null;
 modelAccounting?:{
 usageComplete?:boolean;primaryTokens?:NumberLike;
 auxiliaryObservedTokens?:NumberLike;observedTotalTokens?:NumberLike;
 };
}
export type MessageUsage={
 promptTokens?:NumberLike;cachedPromptTokens?:NumberLike;
 completionTokens?:NumberLike;totalTokens?:NumberLike;estimated?:boolean
}
export const numberOrNull=(value:unknown):number|null=>
 typeof value==='number'&&Number.isFinite(value)&&value>=0?value:null
const first=(...values:unknown[]):number|null=>{
 for(const value of values){const n=numberOrNull(value);if(n!==null)return n}
 return null
}
const sumOrNull=(values:unknown[]):number|null=>{
 if(!values.length)return null
 let total=0
 for(const value of values){const n=numberOrNull(value);if(n===null)return null;total+=n}
 return total
}
const sumAvailable=(...values:unknown[]):number|null=>{
 const valid=values.map(numberOrNull).filter((x):x is number=>x!==null)
 return valid.length?valid.reduce((a,b)=>a+b,0):null
}
export type DetailedCall={
 index:number;prompt:number|null;cached:number|null;output:number|null;
 exact:boolean;estimated:boolean
}
export function executionTokenDetails(record:ExecutionEvidence|null|undefined, fallback?:MessageUsage){
 const ctx=record?.inputContext
 const rounds=Array.isArray(record?.providerRounds)&&record.providerRounds.length
  ?record.providerRounds:Array.isArray(ctx?.modelCallUsage)?ctx.modelCallUsage:[]
 const calls:DetailedCall[]=rounds.map((call,index)=>({
  index:first(call.completedCallIndex,call.round)??index+1,
  prompt:first(call.promptTokens,call.estimatedPromptTokens,call.nativeInputTokensEstimated),
  cached:call.usageExact===true?numberOrNull(call.cachedPromptTokens):null,
  output:first(call.completionTokens,call.outputTokens,call.estimatedOutputTokens),
  exact:call.usageExact===true,
  estimated:call.usageExact===false,
 }))
 const exactCalls=calls.filter(call=>call.exact).length
 const estimatedCalls=calls.filter(call=>call.estimated).length
 const unknownCalls=calls.length-exactCalls-estimatedCalls
 const wire=ctx?.preAdapterWireProfile||ctx?.wireRounds?.[0]?.breakdown
 const parts=wire?.components||{}
 const system=first(ctx?.systemTokens,parts.systemAndPersona)
 const history=first(ctx?.historyTokens,sumAvailable(parts.ownerAndHistory,parts.peerAndSelfHistory))
 const retrieval=first(ctx?.retrievalTokens,ctx?.memoryTokens,parts.retrievedEvidence)
 const schema=first(ctx?.toolSchemaTokens,parts.toolSchemas)
 const firstCall=calls[0]
 const firstInput=first(firstCall?.prompt,wire?.totalEstimatedTokens,ctx?.totalInputTokens,ctx?.wireRounds?.[0]?.breakdown?.totalEstimatedTokens)
 const firstInputExact=firstCall?.exact===true&&firstCall.prompt!==null
 const input=first(sumOrNull(calls.map(call=>call.prompt)),ctx?.providerPromptTokens,fallback?.promptTokens)
 const output=first(sumOrNull(calls.map(call=>call.output)),ctx?.providerOutputTokens,fallback?.completionTokens)
 // A missing cache receipt is UNKNOWN, never a zero cache hit.
 const cached=first(
  calls.length&&calls.every(call=>call.exact&&call.cached!==null)
   ?sumOrNull(calls.map(call=>call.cached)):null,
  ctx?.providerCachedPromptTokens,
  !fallback?.estimated?fallback?.cachedPromptTokens:null,
 )
 const newInput=first(ctx?.providerNewPromptTokens,input!==null&&cached!==null?input-cached:null)
 const recordedTotal=first(record?.tokens,fallback?.totalTokens)
 const tools=first(rounds[0]?.toolsAdvertised,wire?.toolDefinitions,
  Array.isArray(record?.discoveredTools)?record.discoveredTools.length:null)
 const modelCalls=first(record?.modelCalls,calls.length||null)
 const allExact=calls.length>0&&exactCalls===calls.length
 const status=allExact?'measured':estimatedCalls>0?'estimated':'unknown'
 return {calls,exactCalls,estimatedCalls,unknownCalls,allExact,status,
  firstInput,firstInputExact,modelCalls,tools,
  system,history,retrieval,schema,reserved: numberOrNull(ctx?.reservedOutputTokens),
  input,output,cached,newInput,recordedTotal,
  policy:ctx?.policy||record?.contextPolicy||null,overallStatus:record?.status||null,
  accounting:record?.modelAccounting,wire,record,
 }
}
