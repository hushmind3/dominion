// Text action adapter for small local models without native function calling.
const clip=(v,n)=>String(v??'').slice(0,n);
const isObject=x=>x!==null&&typeof x==='object'&&!Array.isArray(x);
const builtins=new Set(['workspace_list','workspace_read','workspace_write','terminal_exec','web_search','memory_store','memory_search','skill_propose']);
const discover={name:'tool_discover',description:'Find available tools, including connected MCP integrations',parameters:{type:'object',properties:{query:{type:'string',description:'What to search for'}},required:['query']}};
const schema=t=>({name:t.name,description:clip(t.description||'',125),arguments:Object.fromEntries(Object.entries(t.parameters?.properties||{}).map(([key,value])=>[key,clip(value.description||value.type||'',80)])),required:t.parameters?.required||[]});
export const selectCompatTools=(tools=[],extra=[])=>[discover,...tools.filter(t=>builtins.has(t.name)||extra.includes(t.name))].slice(0,18);
export function discoverCompatibleTools(tools=[],query=''){
 const words=String(query).toLowerCase().split(/\s+/).filter(Boolean);
 const sorted=tools.map(t=>({t,score:words.reduce((s,w)=>s+(t.name.toLowerCase().includes(w)?3:0)+((t.description||'').toLowerCase().includes(w)?1:0),0)})).sort((a,b)=>b.score-a.score);
 return sorted.filter(x=>x.score).concat(sorted.filter(x=>!x.score)).slice(0,12).map(x=>schema(x.t));
}
export function buildCompatMessages({system='',history=[],tools=[],extra=[],feedback=''}) {
 const lines=[
  'You are an AI assistant using a REAL tool executor, even if you cannot call functions natively.',
  'Return ONE JSON OBJECT only. No markdown, no commentary, no other text.',
  'To perform an action: {"action":"tool","name":"workspace_list","arguments":{"path":""}}',
  'To finish: {"action":"final","message":"Answer in the same language as the user."}',
  'Call ONE tool at a time. Wait for its actual result in the next message. Never claim actions succeeded without tool results.',
  'If a needed tool is not listed, call tool_discover with a short query and use its returned tool.',
  'Use only real tool names and argument keys. Prefer to act rather than merely describe actions.',
  'TOOLS: '+JSON.stringify(selectCompatTools(tools,extra).map(schema)),
  'AGENT ROLE: '+clip(system,1700),
  ...(feedback?['FIX YOUR PRIOR OUTPUT: '+clip(feedback,1200)]:[])
 ];
 const last=history.slice(-10).map(m=>{
  if(m.role==='tool')return {role:'user',content:'[TOOL RESULT '+clip(m.tool_call_id,80)+'] '+clip(m.content,1000)};
  if(m.role==='assistant'&&m.tool_calls?.length)return {role:'assistant',content:'[TOOL CALL] '+m.tool_calls.map(c=>clip(c.function?.name,100)+' '+clip(c.function?.arguments,550)).join('\n')};
  return {role:m.role==='system'?'user':m.role,content:clip(m.content,2200)};
 });
 let remaining=7000;const short=[];
 for(let i=last.length-1;i>=0;i--){
  if(remaining<=0)break;
  const m=last[i],part=m.content.slice(-remaining);
  short.unshift({...m,content:part});remaining-=part.length;
 }
 return [{role:'system',content:lines.join('\n')},...short];
}
function extract(raw){
 let s=clip(raw,50000).trim();
 if(s.startsWith('\x60\x60\x60'))s=s.replace(/^\x60{3}(?:json)?\s*/i,'').replace(/\s*\x60{3}$/,'').trim();
 if(s.startsWith('<tool_call>'))s=s.replace(/^<tool_call>\s*/,'').replace(/\s*<\/tool_call>$/,'').trim();
 return JSON.parse(s);
}
export function parseCompatAction(raw,tools=[]){
 let value;
 try{value=extract(raw);}catch{return {kind:'invalid',reason:'Return exactly one valid JSON action (tool or final).'};}
 if(!isObject(value))return {kind:'invalid',reason:'Action must be a JSON object'};
 if(value.action==='final'){
  if(typeof value.message!=='string'||!value.message.trim())return {kind:'invalid',reason:'final.message must be nonempty text'};
  return {kind:'final',content:value.message};
 }
 if(value.action!=='tool')return {kind:'invalid',reason:'action must be tool or final'};
 if(typeof value.name!=='string'||!isObject(value.arguments))return {kind:'invalid',reason:'tool action requires a tool name and arguments object'};
 const tool=tools.find(t=>t.name===value.name);
 if(!tool)return {kind:'invalid',reason:'Unknown tool '+clip(value.name,100)+'. Use tool_discover if needed.'};
 const fields=tool.parameters?.properties||{},required=tool.parameters?.required||[];
 for(const key of required)if(!(key in value.arguments))return {kind:'invalid',reason:'Missing required argument '+key};
 for(const [key,arg] of Object.entries(value.arguments)){
  const field=fields[key];if(!field)return {kind:'invalid',reason:'Unknown argument '+key+' for '+tool.name};
  if(field.type==='string'&&typeof arg!=='string')return {kind:'invalid',reason:'Argument '+key+' must be text'};
  if(field.type==='array'&&!Array.isArray(arg))return {kind:'invalid',reason:'Argument '+key+' must be an array'};
  if(field.type==='number'&&typeof arg!=='number')return {kind:'invalid',reason:'Argument '+key+' must be numeric'};
  if(field.type==='boolean'&&typeof arg!=='boolean')return {kind:'invalid',reason:'Argument '+key+' must be boolean'};
  if(field.type==='object'&&!isObject(arg))return {kind:'invalid',reason:'Argument '+key+' must be an object'};
 }
 return {kind:'tool',name:tool.name,arguments:value.arguments};
}
export function likelyRequiresTools(text){
 const s=String(text||'');
 return /(?:파일|폴더|디렉터리|코드|프로젝트|저장소|문서|로그|테스트|명령어|터미널|workspace|file|directory|repository|script|test).{0,45}(?:읽어|열어|보여|만들|생성|작성|수정|변경|삭제|실행|검사|확인해|정리해|찾아|저장|list|read|write|edit|create|delete|run|check)/i.test(s)
 || /(?:만들|생성|작성|수정|변경|삭제|실행|검사|저장|열어|읽어).{0,35}(?:파일|폴더|디렉터리|코드|프로젝트|문서|로그|테스트|명령어|터미널)/i.test(s);
}
