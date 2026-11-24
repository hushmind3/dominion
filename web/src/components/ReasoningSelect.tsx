import type {ModelOption,Agent,Provider} from '../api'
import {useInferenceSupport,useRoomInferenceSupport} from '../hooks/useInferenceSupport'
import {selectableEfforts,commonEfforts,roomInferenceRoutes,type InferenceRoute} from '../inferenceControls'
import {SelectInput} from './Primitives'
import {useLocale} from '../i18n'

export function reasoningOptions(model?:ModelOption, _requireCatalog=false):string[] {
 return [...new Set(model?.reasoningEfforts ?? [])]
}
export function ReasoningSelect({value,onChange,model,requireCatalog=false,disabled=false,className='',label,defaultLabel}:{
 value:string;onChange:(value:string)=>void;model?:ModelOption;requireCatalog?:boolean;disabled?:boolean;className?:string;label?:string;defaultLabel?:string
}) {
 const en=useLocale()==='en'
 const labels:Record<string,string>=en?{none:'Off',minimal:'Minimal',low:'Low',medium:'Medium',high:'High',xhigh:'Extra high',max:'Maximum',ultra:'Ultra'}:{none:'끄기',minimal:'최소',low:'낮음',medium:'보통',high:'높음',xhigh:'매우 높음',max:'최대',ultra:'울트라'}
 const options=reasoningOptions(model,requireCatalog)
 return <SelectInput aria-label={label||(en?'Reasoning effort':'추론 강도')} value={value} onChange={e=>onChange(e.target.value)} disabled={disabled} className={className}>
  <option value="">{defaultLabel||(en?'Use default':'기본값 사용')}{model?.defaultReasoningEffort?` (${labels[model.defaultReasoningEffort]||model.defaultReasoningEffort})`:''}</option>
  {value&&!options.includes(value)&&<option value={value} disabled>{labels[value]||value} · {en?'unavailable for this model':'현재 모델 지원 미확인'}</option>}
  {options.map(effort=><option key={effort} value={effort}>{labels[effort]||effort}</option>)}
 </SelectInput>
}

type RouteSelectProps={providerId:string;modelId:string;model?:ModelOption;value:string;onChange:(value:string)=>void;disabled?:boolean;className?:string;defaultLabel?:string}
export function RouteReasoningSelect({providerId,modelId,model,value,onChange,disabled=false,className='',defaultLabel}:RouteSelectProps){
 const en=useLocale()==='en'
 const remote=useInferenceSupport(providerId,modelId)
 const efforts=selectableEfforts(remote.support)
 return <div className="min-w-0">
  <ReasoningSelect value={value} onChange={onChange} model={{id:modelId,reasoningEfforts:efforts,defaultReasoningEffort:model?.defaultReasoningEffort}} requireCatalog disabled={disabled||remote.loading} className={className} defaultLabel={defaultLabel}/>
  {remote.loading?<small className="text-xs text-muted" role="status">{en?'Checking route…':'모델 실행 설정 확인 중…'}</small>:
   remote.error?<small className="text-xs text-rose-700" role="alert">{remote.error}</small>:
   !efforts.length?<small className="text-xs text-muted">{en?'No verified override. Save the connection/model first or use its default.':'확인된 변경 옵션이 없습니다. 연결·모델을 먼저 저장하거나 기본값을 사용하세요.'}</small>:null}
 </div>
}
export function RoomReasoningSelect({room,agents,providers,value,onChange,disabled=false,defaultLabel,className=''}:{
 room:{agentIds:string[];models?:InferenceRoute[];autoModels?:boolean};agents:Agent[];providers:Provider[];value:string;onChange:(value:string)=>void;disabled?:boolean;defaultLabel?:string;className?:string
}){
 const en=useLocale()==='en'
 const routes=roomInferenceRoutes(room,agents,providers)
 const remote=useRoomInferenceSupport(routes)
 const options=commonEfforts(routes,remote.rows)
 return <div className="min-w-0">
  <ReasoningSelect value={value} onChange={onChange} model={{id:'room',reasoningEfforts:options}} requireCatalog disabled={disabled||remote.loading} defaultLabel={defaultLabel} className={className}/>
  <small className="mt-1 block text-xs leading-5 text-muted">{en?'Only levels shared by all known routes are offered. A selected model’s own effort takes precedence.':'모든 실행 모델에 공통으로 지원되는 강도만 표시합니다. 개별 모델 설정이 우선합니다.'}</small>
  {remote.loading?<small role="status" className="text-xs text-muted">{en?'Checking participants…':'참가 모델 확인 중…'}</small>:remote.error?<small role="alert" className="text-xs text-rose-700">{remote.error}</small>:
   !options.length?<small className="text-xs text-muted">{en?'No confirmed common setting; use the model defaults.':'확인된 공통 강도가 없습니다. 모델별 기본값을 사용하세요.'}</small>:null}
 </div>
}
