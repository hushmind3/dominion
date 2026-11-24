// Push delivery is best-effort; durable job/alert state is the source of truth.
export async function notifyAccount(env,accountId,kind,send=fetch){
 if(env.ASSISTANT_PUSH_ENABLED!=='1')return 'disabled';
 const bodies={mail:'새 메일 알림이 있습니다.',job:'업무 진행 결과를 확인하세요.',auth:'웹 업무에 인증 또는 확인이 필요합니다.'};
 if(!bodies[kind])return 'unsupported';
 try{
  const rows=await env.DB.prepare("SELECT p.token FROM assistant_push p JOIN app_sessions s ON s.id=p.app_session_id WHERE p.account_id=? AND s.account_id=p.account_id AND s.status='approved' AND s.capability='assistant_remote' AND s.expires_at>? LIMIT 10").bind(accountId,Math.floor(Date.now()/1000)).all();
  const tokens=(rows.results||[]).map(x=>x.token);if(!tokens.length)return 'unregistered';
  const r=await send('https://exp.host/--/api/v2/push/send',{method:'POST',headers:{'content-type':'application/json',...(env.EXPO_PUSH_ACCESS_TOKEN?{authorization:'Bearer '+env.EXPO_PUSH_ACCESS_TOKEN}:{})},body:JSON.stringify(tokens.map(to=>({to,title:'나의 AI 비서',body:bodies[kind],sound:'default',channelId:'assistant-reminders',data:{assistantMessage:true,kind}}))),signal:AbortSignal.timeout(10000)});
  if(!r.ok)return 'failed';const data=(await r.json()).data,tickets=Array.isArray(data)?data:[data];
  for(let i=0;i<tokens.length;i++)if(tickets[i]?.details?.error==='DeviceNotRegistered')await env.DB.prepare('DELETE FROM assistant_push WHERE token=? AND account_id=?').bind(tokens[i],accountId).run();
  return tickets.some(x=>x?.status==='ok')?'submitted':'failed';
 }catch{return 'failed'}
}
