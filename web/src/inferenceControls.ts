export type InferenceRoute={providerId:string;model:string}
export type InferenceSupport=InferenceRoute&{thinkingToggle:boolean;reasoningEfforts:string[]}
export function routeKey(route:InferenceRoute):string{return JSON.stringify([route.providerId,route.model])}
/** Saved 'none' uses the route's supported boolean off control. */
export function selectableEfforts(support?:InferenceSupport|null):string[]{
 if(!support)return []
 return [...new Set([...(support.thinkingToggle?['none']:[]),...support.reasoningEfforts])]
}
export function commonEfforts(routes:InferenceRoute[],supports:InferenceSupport[]):string[]{
 if(!routes.length)return []
 const byRoute=new Map(supports.map(row=>[routeKey(row),row]))
 return selectableEfforts(byRoute.get(routeKey(routes[0]))).filter(effort=>
  routes.every(route=>selectableEfforts(byRoute.get(routeKey(route))).includes(effort)))
}
/** Explicit rotation pool precedes participant routes. Unknown routing never
 * claims a universal override. */
export function roomInferenceRoutes(
 room:{models?:InferenceRoute[];agentIds:string[];autoModels?:boolean},
 agents:{id:string;providerId:string;model:string}[],
 providers:{id:string;model?:string;enabled:boolean}[],
):InferenceRoute[]{
 const route=(providerId:string,model:string|undefined)=>{
  const provider=providers.find(row=>row.id===providerId&&row.enabled!==false)
  return provider&&model?{providerId,model}:null
 }
 const explicit=room.models||[]
 const chosen=explicit.length?explicit.map(item=>route(item.providerId,item.model)):
  room.agentIds.map(id=>{
   const agent=agents.find(row=>row.id===id)
   if(!agent)return null
   const model=agent.model||(room.autoModels===false?providers.find(p=>p.id===agent.providerId)?.model:undefined)
   return route(agent.providerId,model)
  })
 if(!chosen.length||chosen.some(item=>!item))return []
 return [...new Map(chosen.filter((item):item is InferenceRoute=>!!item).map(item=>[routeKey(item),item])).values()]
}
