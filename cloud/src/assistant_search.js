// Read-only public web search; provider and cost settings are operator-owned.
const reply=(data,status=200)=>Response.json(data,{status,headers:{'cache-control':'no-store'}});
const fail=(error,status=400)=>reply({error},status);
const providers=['exa','ceramic','linkup'];
export function searchReady(env){return env.ASSISTANT_SEARCH_ENABLED==='1'&&Boolean(env.DB&&typeof env.AI?.websearch==='function'&&providers.includes(env.ASSISTANT_SEARCH_PROVIDER)&&Number.isInteger(Number(env.ASSISTANT_SEARCH_DAILY_LIMIT))&&Number(env.ASSISTANT_SEARCH_DAILY_LIMIT)>0&&Number(env.ASSISTANT_SEARCH_DAILY_LIMIT)<=1000)}
const clip=(v,n)=>typeof v==='string'?v.trim().slice(0,n):'';
export function normalizeSearch(data){
 const rows=Array.isArray(data?.result?.items)?data.result.items:Array.isArray(data?.items)?data.items:Array.isArray(data?.result)?data.result:Array.isArray(data?.results)?data.results:Array.isArray(data?.result?.results)?data.result.results:null;
 if(!rows||data?.success===false)throw Error('Invalid search response');
 const out=[],seen=new Set();
 for(const row of rows.slice(0,20)){
  if(typeof row?.url!=='string'||row.url.length>2048)continue;
  let u;try{u=new URL(row.url)}catch{continue}
  if(!['http:','https:'].includes(u.protocol)||u.username||u.password||seen.has(u.href))continue;
  seen.add(u.href);out.push({title:clip(row.title,240)||u.hostname,url:u.href,description:clip(row.description??row.snippet??row.text,700),date:clip(row.lastModifiedDate??row.publishedDate??row.date,80)});
  if(out.length===10)break;
 }return out;
}
export async function searchRoute(req,env,{account}){
 const user=await account(req,env);
 if(!user||user.scope!=='app'||user.capability!=='assistant_remote')return fail('Assistant search permission required',403);
 if(req.method!=='POST'||new URL(req.url).pathname!=='/v1/assistant/search')return fail('Not found',404);
 if(!searchReady(env))return fail('검색 서비스가 서버에서 활성화되지 않았습니다.',503);
 let body;try{const raw=await req.text();if(raw.length>4096)return fail('Search request too large',413);body=JSON.parse(raw)}catch{return fail('Invalid request')}
 const query=clip(body?.query,1025);if(!query||query.length>1024)return fail('Invalid query');
 const limit=Number.isInteger(body.limit)?Math.min(10,Math.max(1,body.limit)):5;
 // Charge admission globally and atomically before sending any request to the provider.
 const day=new Date().toISOString().slice(0,10),cap=Number(env.ASSISTANT_SEARCH_DAILY_LIMIT);
 const reservation=await env.DB.prepare('INSERT INTO assistant_search_quota(day,used) VALUES(?,1) ON CONFLICT(day) DO UPDATE SET used=used+1 WHERE used<? RETURNING used').bind(day,cap).first();
 if(!reservation)return fail('오늘의 검색 사용량 한도에 도달했습니다.',429);
 try{
  const response=await env.AI.websearch({gatewayId:env.ASSISTANT_SEARCH_GATEWAY||'default',provider:env.ASSISTANT_SEARCH_PROVIDER,query,limit});
  if(!response.ok)return fail('검색 공급자 오류 ('+response.status+')',502);
  return reply({query,items:normalizeSearch(await response.json()).slice(0,limit),provider:env.ASSISTANT_SEARCH_PROVIDER,searchedAt:new Date().toISOString()});
 }catch{return fail('실시간 검색에 실패했습니다.',502)}
}
