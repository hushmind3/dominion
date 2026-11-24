import {googleReady,gmailAccess,seal,unseal} from './assistant_google.js';
import {notifyAccount} from './assistant_notify.js';
const now=()=>Math.floor(Date.now()/1000),INTERVAL=900,MAX_IDS=20;
const json=(x,status=200)=>Response.json(x,{status,headers:{'cache-control':'no-store'}});
const fail=(error,status=400)=>json({error},status);
const activeSession="EXISTS(SELECT 1 FROM app_sessions s WHERE s.id=assistant_mail_watch.app_session_id AND s.account_id=assistant_mail_watch.account_id AND s.status='approved' AND s.capability='assistant_remote' AND s.expires_at>?)";
const idOK=x=>typeof x==='string'&&/^[A-Za-z0-9_-]{1,200}$/.test(x);
const cursorOK=x=>typeof x==='string'&&/^\d{1,40}$/.test(x);
const clip=(x,n)=>typeof x==='string'?x.trim().slice(0,n):'';
export function watchFilters(x){
 const mode=x?.mode;
 if(!['all','important','matched'].includes(mode))throw Error('알림 대상을 선택해 주세요.');
 const sender=clip(x.sender,160),keyword=clip(x.keyword,100);
 if(mode==='matched'&&!sender&&!keyword)throw Error('발신자 또는 제목 검색어를 입력하세요.');
 return {mode,sender:mode==='matched'?sender:'',keyword:mode==='matched'?keyword:''};
}
export function mailHeader(m){
 const h=k=>clip(m.payload?.headers?.find(x=>String(x.name).toLowerCase()===k)?.value,300);
 const labels=Array.isArray(m.labelIds)?m.labelIds:[];
 return {id:m.id,from:h('from'),subject:h('subject')||'(제목 없음)',date:h('date'),category:labels.includes('CATEGORY_PROMOTIONS')?'프로모션':labels.includes('CATEGORY_SOCIAL')?'소셜':labels.includes('CATEGORY_UPDATES')?'업데이트':labels.includes('CATEGORY_FORUMS')?'포럼':'일반',important:labels.includes('IMPORTANT'),unread:labels.includes('UNREAD'),inbox:labels.includes('INBOX')};
}
export function matchesMail(m,f){
 if(!m.inbox)return false;
 if(f.mode==='all')return true;if(f.mode==='important')return m.important;
 return (!f.sender||m.from.toLowerCase().includes(f.sender.toLowerCase()))&&(!f.keyword||m.subject.toLowerCase().includes(f.keyword.toLowerCase()));
}
export const mailWatchReady=e=>e.ASSISTANT_MAIL_WATCH_ENABLED==='1'&&googleReady(e);
async function gmail(path,access,send){
 const r=await send('https://gmail.googleapis.com/gmail/v1/users/me/'+path,{headers:{authorization:'Bearer '+access},redirect:'error',signal:AbortSignal.timeout(12000)});
 if(!r.ok)throw Object.assign(Error(r.status===401||r.status===403?'reauthorize':r.status===404?'not_found':'provider_unavailable'),{status:r.status});return r.json();
}
const summary=r=>r?{enabled:r.state==='active',state:r.state,filters:JSON.parse(r.filters),lastCheck:r.last_check?1000*r.last_check:null,nextCheck:1000*r.next_check,error:r.error||null}:{enabled:false,state:'disabled',filters:{mode:'important',sender:'',keyword:''}};
export async function mailWatchRoute(req,env,{account,send=fetch,ctx}){
 const user=await account(req,env);if(user?.scope!=='app'||user.capability!=='assistant_remote'||!user.id.startsWith('google:'))return fail('Assistant mail permission required',403);
 if(!mailWatchReady(env))return fail('메일 감시 서버가 아직 활성화되지 않았습니다.',503);
 const p=new URL(req.url).pathname.slice('/v1/assistant/mail/'.length);
 if(p==='status'&&req.method==='GET')return json(summary(await env.DB.prepare('SELECT * FROM assistant_mail_watch WHERE account_id=?').bind(user.id).first()));
 if(p==='alerts'&&req.method==='GET'){
  const rows=await env.DB.prepare('SELECT id,cipher,created_at FROM assistant_mail_alerts WHERE account_id=? AND created_at>? ORDER BY created_at DESC,id DESC LIMIT 100').bind(user.id,now()-2592000).all();
  const items=[];for(const row of rows.results||[])try{items.push({...JSON.parse(await unseal(env,user.id,row.cipher)),id:row.id,time:row.created_at*1000})}catch{/* corrupted entries never become fabricated notifications */}
  return json({alerts:items.reverse()});
 }
 if(req.method!=='POST')return fail('Not found',404);
 if(p==='stop'){await env.DB.prepare('DELETE FROM assistant_mail_watch WHERE account_id=?').bind(user.id).run();return json({ok:true})}
 if(p==='clear'){await env.DB.prepare('DELETE FROM assistant_mail_alerts WHERE account_id=?').bind(user.id).run();return json({ok:true})}
 if(p==='check'){
  if(ctx)ctx.waitUntil(drainMailWatch(env,user.id,send,true));else await drainMailWatch(env,user.id,send,true);return json({ok:true},202);
 }
 if(p!=='start')return fail('Not found',404);
 try{
  const raw=await req.text();if(raw.length>1024)return fail('Request too large',413);const input=JSON.parse(raw);if(input.confirmed!==true)return fail('메일 감시를 승인해 주세요.');
  const filters=watchFilters(input),{access,grant}=await gmailAccess(env,user.id,send);
  const profile=await gmail('profile?fields=historyId',access,send);
  if(!cursorOK(profile.historyId))throw Error('invalid_history');
  const t=now();
  // Starting/restarting explicitly establishes a fresh baseline, not a replay of old inbox mail.
  const r=await env.DB.prepare("INSERT INTO assistant_mail_watch(account_id,app_session_id,cursor,filters,state,next_check,created_at) SELECT ?,?,?,?,'active',?,? WHERE EXISTS(SELECT 1 FROM assistant_google_grants WHERE account_id=? AND refresh_cipher=?) AND EXISTS(SELECT 1 FROM app_sessions WHERE id=? AND account_id=? AND status='approved' AND capability='assistant_remote' AND expires_at>?) ON CONFLICT(account_id) DO UPDATE SET app_session_id=excluded.app_session_id,cursor=excluded.cursor,filters=excluded.filters,state='active',lease=NULL,lease_until=0,next_check=excluded.next_check,last_check=NULL,error=NULL,created_at=excluded.created_at").bind(user.id,user.app_session_id,profile.historyId,JSON.stringify(filters),t+60,t,user.id,grant,user.app_session_id,user.id,t).run();
  return r.meta.changes?json({ok:true}):fail('Google 연결이 변경되었습니다.',409);
 }catch(e){return fail(e?.message==='reauthorize'?'Gmail 조회 권한을 다시 연결해 주세요.':e?.message?.includes('알림 대상')||e?.message?.includes('발신자')?e.message:'메일 감시 시작 실패',e?.message==='reauthorize'?409:400)}
}
export async function drainMailWatch(env,accountId=null,send=fetch,manual=false){
 if(!mailWatchReady(env))return;
 const t=now();
 await env.DB.prepare('DELETE FROM assistant_mail_alerts WHERE created_at<=?').bind(t-2592000).run();
 const due=await env.DB.prepare("SELECT * FROM assistant_mail_watch WHERE state='active' AND lease_until<=? AND (? IS NULL OR account_id=?) AND (next_check<=? OR (?=1 AND COALESCE(last_check,created_at)<=?)) ORDER BY next_check LIMIT 3").bind(t,accountId,accountId,t,manual?1:0,t-60).all();
 for(const row of due.results||[]){
  const lease=crypto.randomUUID(),claimed=await env.DB.prepare("UPDATE assistant_mail_watch SET lease=?,lease_until=?,last_check=? WHERE account_id=? AND state='active' AND lease_until<=? AND cursor=? AND "+activeSession+" RETURNING account_id").bind(lease,t+180,t,row.account_id,t,row.cursor,t).first();
  if(!claimed){
   await env.DB.prepare("UPDATE assistant_mail_watch SET state='reauthorize',error='앱 로그인 만료 또는 해제' WHERE account_id=? AND state='active' AND NOT "+activeSession).bind(row.account_id,t).run();continue;
  }
  const finish=async(state,error=null,cursor=row.cursor,delay=INTERVAL)=>env.DB.prepare("UPDATE assistant_mail_watch SET state=?,error=?,cursor=?,lease=NULL,lease_until=0,next_check=? WHERE account_id=? AND lease=?").bind(state,error,cursor,now()+delay,row.account_id,lease).run();
  try{
   const {access,grant}=await gmailAccess(env,row.account_id,send);
   const q=new URLSearchParams({startHistoryId:row.cursor,historyTypes:'messageAdded',labelId:'INBOX',maxResults:'10'});
   let history;try{history=await gmail('history?'+q,access,send)}catch(e){if(e?.status===404){await finish('needs_resync','Gmail 변경 기록이 만료되었습니다. 받은편지함을 확인한 뒤 감시를 다시 시작하세요.');continue}throw e}
   const records=Array.isArray(history.history)?history.history:[],ids=[...new Set(records.flatMap(h=>(h.messagesAdded||[]).map(x=>x.message?.id)).filter(idOK))];
   // Never silently drop a high-volume page or skip an unprocessed cursor.
   if(ids.length>MAX_IDS){await finish('needs_resync','한 번에 처리할 새 메일이 너무 많습니다. 받은편지함 확인 후 감시를 다시 시작하세요.');continue}
   const cursor=history.nextPageToken?records.at(-1)?.id:history.historyId;
   if(!cursorOK(cursor)||BigInt(cursor)<BigInt(row.cursor)||history.nextPageToken&&cursor===row.cursor)throw Error('invalid_history');
   const found=[];
   for(let i=0;i<ids.length;i+=4){
    const chunk=await Promise.all(ids.slice(i,i+4).map(async id=>{
     const params=new URLSearchParams({format:'metadata',fields:'id,labelIds,payload(headers)'});for(const h of ['From','Subject','Date'])params.append('metadataHeaders',h);
     try{return mailHeader(await gmail('messages/'+encodeURIComponent(id)+'?'+params,access,send))}catch(e){if(e?.status===404)return null;throw e}
    }));for(const m of chunk)if(m&&idOK(m.id)&&matchesMail(m,JSON.parse(row.filters)))found.push(m);
   }
   // Compare-and-set against the live lease, Google grant, and app session in every write.
   const checks="EXISTS(SELECT 1 FROM assistant_mail_watch WHERE account_id=? AND lease=? AND state='active' AND "+activeSession+") AND EXISTS(SELECT 1 FROM assistant_google_grants WHERE account_id=? AND refresh_cipher=?)";
   const statements=[];for(const m of found)statements.push(env.DB.prepare("INSERT OR IGNORE INTO assistant_mail_alerts(account_id,id,cipher,created_at) SELECT ?,?,?,? WHERE "+checks).bind(row.account_id,m.id,await seal(env,row.account_id,JSON.stringify(m)),now(),row.account_id,lease,now(),row.account_id,grant));
   statements.push(env.DB.prepare("UPDATE assistant_mail_watch SET cursor=?,lease=NULL,lease_until=0,next_check=?,error=NULL WHERE account_id=? AND lease=? AND state='active' AND "+activeSession+" AND EXISTS(SELECT 1 FROM assistant_google_grants WHERE account_id=? AND refresh_cipher=?)").bind(cursor,now()+(history.nextPageToken?60:INTERVAL),row.account_id,lease,now(),row.account_id,grant));
   const results=await env.DB.batch(statements);
   if(results.at(-1)?.meta.changes&&results.slice(0,-1).some(r=>r.meta.changes))await notifyAccount(env,row.account_id,'mail',send);
  }catch(e){await finish(e?.message==='reauthorize'?'reauthorize':'active',e?.message==='reauthorize'?'Gmail 권한 재연결 필요':'메일 조회 실패, 다음 확인 때 재시도',row.cursor,INTERVAL)}
 }
}
