// Owner-scoped inbox and opt-in generic push. Never send private task text to Expo.
const now=()=>Math.floor(Date.now()/1000);
const reply=(x,status=200)=>new Response(JSON.stringify(x),{status,headers:{'content-type':'application/json','cache-control':'no-store'}});
const validToken=t=>typeof t==='string'&&/^(ExpoPushToken|ExponentPushToken)\[[A-Za-z0-9_-]{10,200}\]$/.test(t);
export async function messages(req,env,user,path){
 if(user.scope!=='app'||user.capability!=='assistant_remote')return reply({error:'Assistant mobile permission required'},403);
 if(path==='messages'&&req.method==='GET'){
  const r=await env.DB.prepare('SELECT id,device_id AS deviceId,text,created_at*1000 AS time FROM assistant_messages WHERE account_id=? ORDER BY created_at DESC,id DESC LIMIT 100').bind(user.id).all();
  return reply({messages:r.results.reverse()});
 }
 if(req.method!=='POST')return reply({error:'Method not allowed'},405);
 const raw=await req.text();if(raw.length>1024)return reply({error:'Too large'},413);
 let b;try{b=JSON.parse(raw)}catch{return reply({error:'Invalid JSON'},400)}
 if(!validToken(b?.token))return reply({error:'Invalid Expo push token'},400);
 if(path==='push/unregister'){
  await env.DB.prepare('DELETE FROM assistant_push WHERE token=? AND account_id=? AND app_session_id=?').bind(b.token,user.id,user.app_session_id).run();return reply({ok:true});
 }
 if(path!=='push/register')return reply({error:'Not found'},404);
 // Require a fresh scoped session; token registration is opt-in. Never transfer a
 // push endpoint between accounts without removing its previous registration.
 const old=await env.DB.prepare('SELECT account_id FROM assistant_push WHERE token=?').bind(b.token).first();
 if(old&&old.account_id!==user.id)return reply({error:'Push token belongs to another account'},409);
 const r=await env.DB.prepare("INSERT INTO assistant_push(token,account_id,app_session_id,created_at) SELECT ?,?,?,? WHERE (SELECT COUNT(*) FROM assistant_push WHERE account_id=?)<10 OR EXISTS(SELECT 1 FROM assistant_push WHERE token=? AND account_id=?) ON CONFLICT(token) DO UPDATE SET app_session_id=excluded.app_session_id,created_at=excluded.created_at WHERE assistant_push.account_id=excluded.account_id").bind(b.token,user.id,user.app_session_id,now(),user.id,b.token,user.id).run();
 return r.meta.changes?reply({ok:true}):reply({error:'Push device limit'},429);
}
export async function publish(env,device,b,send=fetch){
 if(typeof b?.id!=='string'||!/^[A-Za-z0-9:_-]{1,160}$/.test(b.id)||typeof b.text!=='string'||!b.text.trim()||b.text.length>2000)return reply({error:'Invalid message'},400);
 const t=now();
 const r=await env.DB.prepare("INSERT OR IGNORE INTO assistant_messages(id,account_id,device_id,text,created_at) SELECT ?,?,?,?,? WHERE (SELECT COUNT(*) FROM assistant_messages WHERE account_id=? AND created_at>?)<100 AND EXISTS(SELECT 1 FROM assistant_devices d LEFT JOIN app_sessions s ON s.id=d.app_session_id WHERE d.id=? AND d.account_id=? AND d.revoked=0 AND (d.app_session_id IS NULL OR (s.status='approved' AND s.expires_at>? AND s.capability='assistant_pc')))").bind(b.id,device.account_id,device.id,b.text,t,device.account_id,t-3600,device.id,device.account_id,t).run();
 if(!r.meta.changes){const old=await env.DB.prepare('SELECT id FROM assistant_messages WHERE device_id=? AND id=?').bind(device.id,b.id).first();return old?reply({ok:true,duplicate:true}):reply({error:'Message limit or revoked'},429)}
 // Push failure does not undo durable inbox delivery. No repeated SMS/call.
 const rows=await env.DB.prepare("SELECT p.token FROM assistant_push p JOIN app_sessions s ON s.id=p.app_session_id WHERE p.account_id=? AND s.account_id=p.account_id AND s.status='approved' AND s.capability='assistant_remote' AND s.expires_at>? LIMIT 10").bind(device.account_id,t).all();
 let push='not_configured';
 if(rows.results.length&&env.ASSISTANT_PUSH_ENABLED==='1'){
  try{const res=await send('https://exp.host/--/api/v2/push/send',{method:'POST',headers:{'content-type':'application/json',...(env.EXPO_PUSH_ACCESS_TOKEN?{authorization:'Bearer '+env.EXPO_PUSH_ACCESS_TOKEN}:{})},body:JSON.stringify(rows.results.map(x=>({to:x.token,title:'나의 AI 비서',body:'새 메시지가 도착했습니다.',sound:'default',data:{assistantMessage:true},channelId:'assistant-reminders'})))});push=res.ok?'submitted':'failed'}catch{push='failed'}
 }
 return reply({ok:true,push});
}
