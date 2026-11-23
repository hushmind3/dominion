/** Frontend-only MCP command grammar. All mutations use the existing MCP API. */
export type McpQuickAction =
 | {kind:'open'} | {kind:'list'} | {kind:'help'}
 | {kind:'search';query:string}
 | {kind:'add';name:string;url:string}
 | {kind:'enable'|'disable'|'remove';name:string}
 | {kind:'invalid';message:string}

export type InstalledMcp = {
 id:string;name:string;enabled?:boolean;transport:'http'|'stdio'
 url?:string;command?:string;args?:string[];apiKey?:string
}
export type RegistryMcp = {
 name:string;description?:string;url?:string;packageName?:string
}
export type McpAvailableTool = {mcpId?:string;name?:string;error?:string}

export function validRemoteMcpUrl(input:string):boolean{
 try {
  const url=new URL(input)
  return url.protocol==='https:' && Boolean(url.hostname)
   && !url.username && !url.password && !url.hash && url.port!=='0'
 }catch{return false}
}

/** Parse /mcp followed by one user-intended operation; never execute a shell. */
export function parseMcpQuickCommand(input:string):McpQuickAction{
 const text=input.trim()
 if(!text)return {kind:'open'}
 const match=text.match(/^(\S+)(?:\s+([\s\S]+))?$/)
 if(!match)return {kind:'invalid',message:'MCP 명령어를 확인하세요.'}
 const sub=match[1].toLowerCase(),arg=(match[2]||'').trim()
 if(['help','?'].includes(sub))return {kind:'help'}
 if(sub==='list'||sub==='ls'||sub==='status')return arg?{kind:'invalid',message:'사용법: /mcp list'}:{kind:'list'}
 if(sub==='search'||sub==='find')return arg?{kind:'search',query:arg}:{kind:'invalid',message:'사용법: /mcp search 검색어'}
 if(sub==='add'||sub==='connect'||sub==='install'){
  const value=arg.match(/^(.+?)\s+(https?:\/\/\S+)$/i)
  if(!value||!value[1].trim())return {kind:'invalid',message:'사용법: /mcp add 이름 https://서버주소/mcp'}
  if(!validRemoteMcpUrl(value[2]))return {kind:'invalid',message:'원격 MCP는 인증정보 없는 HTTPS 주소를 입력하세요.'}
  return {kind:'add',name:value[1].trim(),url:value[2]}
 }
 if(['enable','disable','remove'].includes(sub))
  return arg?{kind:sub as 'enable'|'disable'|'remove',name:arg}:
   {kind:'invalid',message:'사용법: /mcp '+sub+' MCP이름'}
 return {kind:'invalid',message:'지원하지 않는 MCP 명령입니다. /mcp help로 사용법을 확인하세요.'}
}

export function findInstalledMcp(items:InstalledMcp[],name:string):InstalledMcp|undefined{
 const key=name.trim()
 const needle=key.toLocaleLowerCase()
 // Names are owner-facing and IDs are internal. If either matches multiple
 // registrations, refusing the action is safer than mutating the wrong MCP.
 const matches=items.filter(item=>item.id===key||item.name.toLocaleLowerCase()===needle)
 return matches.length===1?matches[0]:undefined
}
