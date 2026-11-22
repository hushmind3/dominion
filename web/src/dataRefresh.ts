import type {RefreshScope} from './refreshEvents'

/** Manual refreshes may name the touched API routes; no arguments preserves
 * the legacy full refresh button behavior. */
export const refresh=(...targets:string[])=>{
 if(!targets.length){window.dispatchEvent(new Event('dominion-refresh'));return}
 window.dispatchEvent(new CustomEvent<RefreshScope>('dominion-data-change',{
  detail:{targets,sessions:[],rooms:[]}
 }))
}
