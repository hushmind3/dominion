import {useEffect} from 'react'
import {connectEngineEvents} from '../services/engineEvents'
import {useRemote} from './useRemote'

type Status={jobs:number;branch?:string;commit?:string}

/** Connection status and conversations share read state and one event bridge. */
export function useEngineStatus() {
 const state=useRemote<Status|null>('/api/status',null)
 const approvals=useRemote<unknown[]>('/api/approvals',[])
 useEffect(()=>{
  const status=()=>{void state.reload()}
  const pending=()=>{void approvals.reload()}
  const disconnect=connectEngineEvents(status,pending)
  const poll=setInterval(()=>{if(!document.hidden)status()},12_000)
  const resume=()=>{if(!document.hidden)status()}
  document.addEventListener('visibilitychange',resume)
  return()=>{disconnect();clearInterval(poll);document.removeEventListener('visibilitychange',resume)}
 },[state.reload,approvals.reload])
 const online=Boolean(state.value&&!state.error)
 return {online,connecting:state.loading,running:online?Number(state.value?.jobs)||0:0,
  pending:approvals.error?0:approvals.value.length,
  engineBuild:state.value?.branch&&state.value.commit?{branch:state.value.branch,commit:state.value.commit}:null}
}
