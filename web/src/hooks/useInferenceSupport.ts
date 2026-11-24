import {useEffect,useState} from 'react'
import {post} from '../api'
import {useRemote} from './useRemote'
import type {InferenceRoute,InferenceSupport} from '../inferenceControls'
import {matchesRefreshTarget,type RefreshScope} from '../refreshEvents'

const endpoint='/api/chat/inference-support'
export function useInferenceSupport(providerId:string,model:string,active=true){
 const remote=useRemote<InferenceSupport|null>(active&&providerId&&model?
  endpoint+'?providerId='+encodeURIComponent(providerId)+'&model='+encodeURIComponent(model):null,null,active)
 const support=!remote.error&&!remote.loading&&remote.value?.providerId===providerId&&remote.value?.model===model?remote.value:null
 return {...remote,support}
}
export function useRoomInferenceSupport(routes:InferenceRoute[]){
 const key=JSON.stringify(routes)
 const [state,setState]=useState<{key:string;rows:InferenceSupport[];loading:boolean;error:string}>({key:'',rows:[],loading:true,error:''})
 useEffect(()=>{
  const parsed=JSON.parse(key) as InferenceRoute[]
  let controller:AbortController|undefined
  const load=()=>{
   controller?.abort()
   if(!parsed.length){setState({key,rows:[],loading:false,error:''});return}
   const request=new AbortController()
   controller=request
   setState({key,rows:[],loading:true,error:''})
   void post<InferenceSupport[]>(endpoint,parsed,request.signal).then(rows=>{
    if(!request.signal.aborted)setState({key,rows,loading:false,error:''})
   }).catch(error=>{
    if(!request.signal.aborted)setState({key,rows:[],loading:false,error:String(error.message||error)})
   })
  }
  const changed=(event:Event)=>{
   if(matchesRefreshTarget(endpoint,(event as CustomEvent<RefreshScope>).detail?.targets||[]))load()
  }
  load()
  window.addEventListener('dominion-data-change',changed)
  return()=>{controller?.abort();window.removeEventListener('dominion-data-change',changed)}
 },[key])
 return state.key===key?state:{key,rows:[],loading:true,error:''}
}
