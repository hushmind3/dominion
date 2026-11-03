import os from 'node:os';
import path from 'node:path';
import {all,get,put,remove,writeMessage,messages,log,bus,db} from './store.mjs';
import {ExecutionRuntime} from './execution-runtime.mjs';
import {streamCompletion,providerById} from './providers.mjs';
import {toolsForModel,executeTool} from './tools.mjs';
import {resolveModel} from './model-router.mjs';
import {connections as oauthConnections} from './oauth.mjs';
export const agents=()=>all('agent');
export const sessions=()=>all('session');
export function newSession(input={}){
 return put('session',{title:input.title||'새 대화',agentId:input.agentId||'',roomId:input.roomId||null,providerId:input.providerId||'',model:input.model||'',lastActivity:new Date().toISOString()});
}
export function sessionMessages(id){if(!get('session',id))throw Error('Session not found');return messages(id);}
function systemFor(agent,goal=''){
 const enabledSkills=all('skill').filter(s=>s.enabled).slice(0,12).map(x=>'### '+x.name+'\n'+x.instructions).join('\n\n');
 const recentMemory=all('memory').filter(x=>x.scope==='shared'||x.scope===agent?.id).slice(0,16).map(x=>'- '+x.text).join('\n');
 const home=os.homedir();
 const paths='Computer paths: home='+home+'; Desktop='+path.join(home,'Desktop')+'; Documents='+path.join(home,'Documents')+'; Downloads='+path.join(home,'Downloads')+'. Use file_list/file_read/file_write with absolute paths for these folders, and shell_exec for real system commands. workspace_* tools only cover the workspace folder.';
 const accounts=oauthConnections().map(account=>account.provider+' ('+account.displayName+')');
 const accountLine='Connected accounts: '+(accounts.length?accounts.join(', '):'none')+'. Use account_status, github_api, youtube_search/youtube_channels and youtube_upload (approval required) for real account actions; never claim account access without a successful tool result.';
 return [agent?.systemPrompt||'You are a capable AI assistant.','You may use available tools. Do not claim to have performed a tool action unless it succeeded.',paths,accountLine,goal?'Current task: '+goal:'',recentMemory?'Relevant memory:\n'+recentMemory:'',enabledSkills?'Installed skills:\n'+enabledSkills:''].filter(Boolean).join('\n\n');
}
function asOpenAI(row,speakerLabels=false){
 const v={role:row.role,content:row.content||''};
 if(row.role==='assistant'){
   if(speakerLabels&&!row.tool_calls&&row.agent_id){const speaker=get('agent',row.agent_id);if(speaker)v.content='[Speaker: '+speaker.name+']\n'+v.content;}
   if(row.reasoning)v.reasoning_content=row.reasoning;
   if(row.tool_calls)v.tool_calls=row.tool_calls;
 }
 if(row.role==='tool')v.tool_call_id=row.tool_call_id;
 return v;
}
function runtimeHistory(sessionId){
 const rows=messages(sessionId);
 const checkpoint=get('session_compaction',sessionId);
 if(!checkpoint)return rows;
 const through=rows.findIndex(row=>row.id===checkpoint.throughMessageId);
 return through===-1?rows:rows.slice(through+1);
}
const executionRuntime=new ExecutionRuntime({
 complete:streamCompletion,discoverTools:toolsForModel,invokeTool:executeTool,
 getToolMode:(providerId,modelId)=>{
  const provider=providerById(providerId);
  if(provider.kind==='anthropic')return 'native';
   if(provider.toolMode)return provider.toolMode;
  if(provider.kind==='ollama'||(provider.local&&/(?:^|[^0-9])(?:[1-8](?:\.\d+)?)b(?:[^a-z]|$)/i.test(modelId)))return 'compat';
  return 'auto';
 },
 readHistory:runtimeHistory,convertMessage:asOpenAI,saveMessage:writeMessage,
 saveRun:run=>{put('execution',run);bus.emit('execution_event',{runId:run.id,sessionId:run.sessionId,status:run.status});},
 saveOperation:operation=>put('execution_operation',operation)
});
export const executions=()=>all('execution');
export const cancelExecution=id=>executionRuntime.cancel(id);
export const activeExecutions=()=>executionRuntime.active();
export function recoverExecutions(){
 for(const run of executions().filter(x=>x.status==='running')){
  put('execution',{...run,status:'interrupted',reason:'Process stopped; inspect unfinished tool calls before replay'});
 }
 for(const op of all('execution_operation').filter(x=>x.status==='started')){
  put('execution_operation',{...op,status:'uncertain',reason:'Interrupted before tool result was persisted'});
 }
}
export async function runTurn({sessionId,agentId,providerId,model,input,systemOverride,reasoningLanguage,reasoningEffort,thinking,onEvent=()=>{},signal,toolsEnabled=true,speakerLabels=false,limits={}}){
 const sess=get('session',sessionId);if(!sess)throw Error('Session not found');
 if(executionRuntime.isActive(sessionId))throw Error('Session is already running');
 const agent=get('agent',agentId||sess.agentId)||null;
 let provider=providerId||sess.providerId||agent?.providerId;
 let modelId=model||sess.model||agent?.model;
 if(!provider||!modelId){
  const selected=await resolveModel({agentId:agent?.id,providerId:provider,model:modelId});
  provider=selected.providerId;modelId=selected.model;
 }
 if(input!==undefined&&String(input).trim()){
  writeMessage({session_id:sessionId,role:'user',content:String(input),agent_id:agent?.id});
  const title=sess.title==='새 대화'?String(input).trim().slice(0,58):sess.title;
  put('session',{...sess,title,providerId:provider,model:modelId,lastActivity:new Date().toISOString()});
 }
 const summary=get('session_compaction',sessionId)?.summary;
 const policy=get('policy','default')||{};
 const access=policy.autoApproveAll?'Full computer access is enabled. Use the real tools directly; do not ask for permission in text.':policy.autoApproveMcp?'Computer actions require approval except MCP tools.':'Computer actions require user approval.';
 const system=(systemOverride||systemFor(agent))+(summary?'\n\nPrevious conversation context summary:\n'+summary:'')+'\n\n'+access+(reasoningLanguage==='ko'?'\n\nWhen the provider exposes visible reasoning content, prefer explaining your thought process in Korean.':'');
 return executionRuntime.execute({sessionId,agentId:agent?.id,providerId:provider,model:modelId,system,onEvent,signal,toolsEnabled,speakerLabels,reasoningEffort,thinking,limits});
}
export function renameSession(id,title){
 const record=get('session',id),name=String(title||'').trim();
 if(!record)throw Error('Session not found');
 if(!name||name.length>120)throw Error('Session name must be 1–120 characters');
 return put('session',{...record,title:name});
}
export function clearSession(id){
 if(!get('session',id))throw Error('Session not found');
 if(executionRuntime.isActive(id))throw Error('Stop the current execution before clearing the session');
 const result=db.prepare('DELETE FROM messages WHERE session_id=?').run(id);
 remove('session_compaction',id);
 return {cleared:Number(result.changes),sessionId:id};
}
export function forkSession(id){
 const original=get('session',id);if(!original)throw Error('Session not found');
 if(executionRuntime.isActive(id))throw Error('Stop the current execution before forking the session');
 const fresh=newSession({title:original.title+' (분기)',agentId:original.agentId,providerId:original.providerId,model:original.model});
 const mapping=new Map();
 for(const row of messages(id)){
  const cloned=writeMessage({session_id:fresh.id,role:row.role,content:row.content,reasoning:row.reasoning,
   tool_calls:row.tool_calls,tool_call_id:row.tool_call_id,agent_id:row.agent_id});
  mapping.set(row.id,cloned.id);
 }
 const checkpoint=get('session_compaction',id);
 if(checkpoint&&mapping.has(checkpoint.throughMessageId))put('session_compaction',
  {id:fresh.id,summary:checkpoint.summary,throughMessageId:mapping.get(checkpoint.throughMessageId)});
 return fresh;
}
export async function compactSession(id,{providerId,model}={}){
 if(!get('session',id))throw Error('Session not found');
 if(executionRuntime.isActive(id))throw Error('Stop the current execution before compacting context');
 if(!providerId||!model)throw Error('Select a connected model for context compaction');
 const history=messages(id),prior=get('session_compaction',id);
 const previousIndex=prior?history.findIndex(row=>row.id===prior.throughMessageId):-1;
 if(prior&&previousIndex<0)throw Error('Previous context checkpoint is missing');
 let cut=Math.max(previousIndex+1,history.length-8);
 while(cut>previousIndex+1&&history[cut]?.role==='tool')cut--;
 if(cut<=previousIndex+1)throw Error('Not enough new messages to compact; continue the conversation first');
 const candidates=history.slice(previousIndex+1,cut);
 const transcript=candidates.map(row=>
  '['+row.role+'] '+String(row.content||'').slice(0,3200)+
  (row.tool_calls?' [tool calls: '+row.tool_calls.map(c=>c.function?.name||'tool').join(', ')+']':'')
 ).join('\n').slice(-110000);
 const response=await streamCompletion({providerId,model,tools:[],messages:[
  {role:'system',content:'Summarize prior conversation for continuation. Preserve user intent, key decisions, files, constraints, open tasks, and important tool outcomes. Do not invent results. Be concise and write in the same language as the conversation.'},
  {role:'user',content:(prior?'Previous summary:\n'+prior.summary+'\n\n':'')+'Conversation to compact:\n'+transcript}
 ]});
 const summary=String(response.content||'').trim().slice(0,18000);
 if(!summary)throw Error('Model returned an empty context summary');
 put('session_compaction',{id,throughMessageId:candidates.at(-1).id,summary,updatedAt:new Date().toISOString()});
 return {sessionId:id,summarized:candidates.length,remaining:history.length-cut,summary,usage:response.usage};
}
function extractJsonObject(text){
 const source=String(text||'').trim(),start=source.indexOf('{');
 if(start<0)return null;
 let depth=0,inString=false,escaped=false;
 for(let i=start;i<source.length;i++){
  const char=source[i];
  if(inString){if(escaped)escaped=false;else if(char==='\\')escaped=true;else if(char==='"')inString=false;continue}
  if(char==='"'){inString=true;continue}
  if(char==='{')depth++;
  else if(char==='}'&&--depth===0){try{return JSON.parse(source.slice(start,i+1))}catch{return null}}
 }
 return null;
}
export async function learnFromSession(id,{providerId,model}={}){
 if(!get('session',id))throw Error('Session not found');
 if(!providerId||!model)throw Error('Select a connected model for learning');
 const history=messages(id).filter(row=>row.role==='user'||row.role==='assistant');
 if(history.length<2)throw Error('Not enough conversation to learn from');
 const transcript=history.slice(-40).map(row=>'['+row.role+'] '+String(row.content||'').slice(0,3000)).join('\n').slice(-30000);
 const response=await streamCompletion({providerId,model,tools:[],messages:[
  {role:'system',content:'Extract durable knowledge from the conversation. Return ONE JSON object only: {"memories":[{"text":"...","scope":"shared"}],"skill":{"name":"...","instructions":"..."}} . Use null or an empty array when there is nothing durable. Memories must be stable user preferences, facts, decisions, or project constraints. Never include secrets, temporary details, tool output, or speculation. Skill instructions must be reusable and concise.'},
  {role:'user',content:'Conversation:\n'+transcript}
 ]});
 const data=extractJsonObject(response.content);
 if(!data)throw Error('The model did not return valid learning JSON');
 const memories=(Array.isArray(data.memories)?data.memories:[]).filter(item=>item&&typeof item.text==='string'&&item.text.trim()).slice(0,10).map(item=>put('memory',{text:String(item.text).trim().slice(0,1000),scope:String(item.scope||'shared')}));
 let skill=null;
 if(data.skill&&typeof data.skill.name==='string'&&typeof data.skill.instructions==='string'&&data.skill.name.trim()&&data.skill.instructions.trim()){
  skill=put('skill',{name:data.skill.name.trim().slice(0,80),instructions:data.skill.instructions.trim().slice(0,12000),enabled:false,status:'proposed'});
 }
 return {sessionId:id,memories,skill,usage:response.usage};
}
export function rooms(){return all('room');}
export function saveRoom(data){
 const ids=Array.isArray(data.agentIds)?[...new Set(data.agentIds.map(String))]:[];
 return put('room',{id:data.id||crypto.randomUUID(),name:String(data.name||'새 회의실'),agentIds:ids,rounds:Math.max(1,Math.min(4,Number(data.rounds)||1)),maxTurns:Math.max(1,Math.min(60,Number(data.maxTurns)||12)),sessionId:data.sessionId||null,description:String(data.description||'')});
}
export async function runMeeting(roomId,goal,onEvent=()=>{}){
 let room=get('room',roomId);if(!room)throw Error('Room not found');
 if(!room.agentIds?.length)throw Error('Add at least one agent to this room');
 const sessionId=room.sessionId||newSession({title:room.name,roomId}).id;
 if(!room.sessionId){room=put('room',{...room,sessionId});}
 writeMessage({session_id:sessionId,role:'user',content:'회의 의제: '+goal});
 let turnCount=0;
 for(let round=0;round<room.rounds;round++){
  for(const aid of room.agentIds){
   if(++turnCount>Math.max(1,Number(room.maxTurns)||12))break;
   const agent=get('agent',aid);if(!agent){onEvent({type:'skipped',agentId:aid});continue;}
   onEvent({type:'speaker',agentId:aid,round:round+1,name:agent.name});
   const prompt=systemFor(agent,'You are participant '+agent.name+' in a roundtable. Address the shared goal, react to prior speakers, and contribute an independent perspective. Do not impersonate other agents.');
   await runTurn({sessionId,agentId:aid,providerId:agent.providerId,model:agent.model,systemOverride:prompt,onEvent,speakerLabels:true});
  }
 }
 log('meeting.completed',{roomId,sessionId});return {sessionId,roomId};
}
