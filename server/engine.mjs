import {all,get,put,writeMessage,messages,log} from './store.mjs';
import {streamCompletion} from './providers.mjs';
import {toolsForModel,executeTool} from './tools.mjs';
export function agents(){
 const xs=all('agent');
 if(!xs.length){put('agent',{id:'general',name:'General',description:'범용 작업 에이전트',systemPrompt:'You are a capable, practical assistant. Answer in the user language. Use tools to accomplish tasks when available.',providerId:'',model:'',color:'#5879e9'});return all('agent');}
 return xs;
}
export const sessions=()=>all('session');
export function newSession(input={}){
 return put('session',{title:input.title||'새 대화',agentId:input.agentId||'general',roomId:input.roomId||null,providerId:input.providerId||'',model:input.model||'',lastActivity:new Date().toISOString()});
}
export function sessionMessages(id){if(!get('session',id))throw Error('Session not found');return messages(id);}
function systemFor(agent,goal=''){
 const enabledSkills=all('skill').filter(s=>s.enabled).slice(0,12).map(x=>'### '+x.name+'\n'+x.instructions).join('\n\n');
 const recentMemory=all('memory').filter(x=>x.scope==='shared'||x.scope===agent?.id).slice(0,16).map(x=>'- '+x.text).join('\n');
 return [agent?.systemPrompt||'You are a capable AI assistant.','You may use available tools. Do not claim to have performed a tool action unless it succeeded.',goal?'Current task: '+goal:'',recentMemory?'Relevant memory:\n'+recentMemory:'',enabledSkills?'Installed skills:\n'+enabledSkills:''].filter(Boolean).join('\n\n');
}
function asOpenAI(row){
 const v={role:row.role,content:row.content||''};
 if(row.role==='assistant'){
   if(row.reasoning)v.reasoning_content=row.reasoning;
   if(row.tool_calls)v.tool_calls=row.tool_calls;
 }
 if(row.role==='tool')v.tool_call_id=row.tool_call_id;
 return v;
}
const runningSessions=new Set();
export async function runTurn({sessionId,agentId,providerId,model,input,systemOverride,onEvent=()=>{},signal,toolsEnabled=true}){
 const sess=get('session',sessionId);if(!sess)throw Error('Session not found');
 if(runningSessions.has(sessionId))throw Error('Session is already running');
 runningSessions.add(sessionId);
 try{
  const agent=get('agent',agentId||sess.agentId)||agents().find(a=>a.id==='general');
  const provider=providerId||sess.providerId||agent?.providerId;
  const modelId=model||sess.model||agent?.model;
  if(!provider||!modelId)throw Error('Select a provider and model in the model picker');
  if(input!==undefined&&String(input).trim()){
    writeMessage({session_id:sessionId,role:'user',content:String(input),agent_id:agent?.id});
    const title=sess.title==='새 대화'?String(input).trim().slice(0,58):sess.title;
    put('session',{...sess,title,providerId:provider,model:modelId,lastActivity:new Date().toISOString()});
  }
  const system=systemOverride||systemFor(agent);
  const tools=await toolsForModel(toolsEnabled);
  let iterations=0;
  while(iterations++<8){
   const history=[{role:'system',content:system},...messages(sessionId).map(asOpenAI)];
   const result=await streamCompletion({providerId:provider,model:modelId,messages:history,tools,onEvent,signal});
   const assistant=writeMessage({session_id:sessionId,role:'assistant',content:result.content,reasoning:result.reasoning_content,tool_calls:result.tool_calls.length?result.tool_calls:null,agent_id:agent?.id});
   onEvent({type:'assistant_saved',message:assistant,usage:result.usage});
   if(!result.tool_calls.length){onEvent({type:'done',sessionId,usage:result.usage});return assistant;}
   for(const call of result.tool_calls){
    const tool=tools.find(x=>x.name===call.function.name);
    let output;
    try{
     if(!tool)throw Error('Tool not enabled: '+call.function.name);
     const args=JSON.parse(call.function.arguments||'{}');
     onEvent({type:'tool_start',name:tool.name,args,callId:call.id});
     output=await executeTool(tool,args,onEvent);
     onEvent({type:'tool_end',name:tool.name,callId:call.id,result:output});
    }catch(e){output={error:String(e.message||e)};onEvent({type:'tool_error',name:call.function.name,error:String(e.message||e)});}
    writeMessage({session_id:sessionId,role:'tool',tool_call_id:call.id,content:typeof output==='string'?output:JSON.stringify(output).slice(0,30000)});
   }
  }
  throw Error('Tool execution reached the 8-step ceiling; continue in a new turn');
 }finally{runningSessions.delete(sessionId);}
}
export function rooms(){return all('room');}
export function saveRoom(data){
 const ids=Array.isArray(data.agentIds)?[...new Set(data.agentIds.map(String))]:[];
 return put('room',{id:data.id||crypto.randomUUID(),name:String(data.name||'새 회의실'),agentIds:ids,rounds:Math.max(1,Math.min(4,Number(data.rounds)||1)),sessionId:data.sessionId||null,description:String(data.description||'')});
}
export async function runMeeting(roomId,goal,onEvent=()=>{}){
 let room=get('room',roomId);if(!room)throw Error('Room not found');
 if(!room.agentIds?.length)throw Error('Add at least one agent to this room');
 const sessionId=room.sessionId||newSession({title:room.name,roomId}).id;
 if(!room.sessionId){room=put('room',{...room,sessionId});}
 writeMessage({session_id:sessionId,role:'user',content:'회의 의제: '+goal});
 for(let round=0;round<room.rounds;round++){
  for(const aid of room.agentIds){
   const agent=get('agent',aid);if(!agent){onEvent({type:'skipped',agentId:aid});continue;}
   onEvent({type:'speaker',agentId:aid,round:round+1,name:agent.name});
   const prompt=systemFor(agent,'You are participant '+agent.name+' in a roundtable. Address the shared goal, react to prior speakers, and contribute an independent perspective. Do not impersonate other agents.');
   await runTurn({sessionId,agentId:aid,providerId:agent.providerId,model:agent.model,systemOverride:prompt,onEvent});
  }
 }
 log('meeting.completed',{roomId,sessionId});return {sessionId,roomId};
}
