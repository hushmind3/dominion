// Durable job/alert state is always the source of truth; push is best-effort.
import {deliverPush} from './assistant_push_delivery.js';
export async function notifyAccount(env,accountId,kind,send=fetch){
 if(env.ASSISTANT_PUSH_ENABLED!=='1')return 'disabled';
 const bodies={mail:'새 메일 알림이 있습니다.',job:'업무 진행 결과를 확인하세요.',auth:'웹 업무에 인증 또는 확인이 필요합니다.'};
 if(!bodies[kind])return 'unsupported';
 try{
  const rows=await env.DB.prepare("SELECT p.token FROM assistant_push p JOIN app_sessions s ON s.id=p.app_session_id WHERE p.account_id=? AND s.account_id=p.account_id AND s.status='approved' AND s.capability='assistant_remote' AND s.expires_at>? LIMIT 10").bind(accountId,Math.floor(Date.now()/1000)).all();
  return deliverPush(env,accountId,rows.results.map(x=>x.token),bodies[kind],kind,{send});
 }catch{return 'failed'}
}
