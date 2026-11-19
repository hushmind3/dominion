// DOMINION email link identity: a mail-link grants no session by itself.
// A separate, secret polling credential on the initiating device receives the session.
// Use Resend as an optional transactional mail adapter; no secrets in the browser.
const epoch=()=>Math.floor(Date.now()/1000);
const TTL=600;
const random=()=>btoa(String.fromCharCode(...crypto.getRandomValues(new Uint8Array(32)))).replace(/\+/g,'-').replace(/\//g,'_').replace(/=/g,'');
const validSecret=value=>typeof value==='string'&&/^[A-Za-z0-9_-]{43}$/.test(value);
const digest=async value=>{
 const buffer=await crypto.subtle.digest('SHA-256',new TextEncoder().encode(value));
 return btoa(String.fromCharCode(...new Uint8Array(buffer))).replace(/\+/g,'-').replace(/\//g,'_').replace(/=/g,'');
};
const reply=(data,status=200)=>new Response(JSON.stringify(data),{status,headers:{'content-type':'application/json; charset=utf-8','cache-control':'no-store'}});
const fail=(message,status=400)=>reply({error:message},status);
const page=(title,text,action='')=>new Response('<!doctype html><html lang="ko"><meta charset="utf-8"><meta name="referrer" content="no-referrer"><meta name="viewport" content="width=device-width,initial-scale=1"><title>'+title+'</title><body style="max-width:32rem;margin:10vh auto;padding:1.5rem;font:16px system-ui;color:#24211d"><h1>'+title+'</h1><p>'+text+'</p>'+action+'</body></html>',{
 headers:{'content-type':'text/html; charset=utf-8','cache-control':'no-store','referrer-policy':'no-referrer','x-content-type-options':'nosniff','content-security-policy':"default-src 'none'; style-src 'unsafe-inline'; form-action 'self'; frame-ancestors 'none'"}});
const cleanEmail=value=>{
 if(typeof value!=='string')return null;
 const email=value.trim().toLowerCase();
 if(email.length<5||email.length>320||!/^[^\s@<>]{1,64}@[a-z0-9.-]{3,253}$/.test(email)||email.includes('..')||email.endsWith('.'))return null;
 return email;
};
export const emailLoginReady=env=>Boolean(env.EMAIL_FROM&&env.DB&&(env.EMAIL?.send||env.RESEND_API_KEY));
const parseJson=async request=>{
 if(Number(request.headers.get('content-length')||0)>2048)return null;
 const text=await request.text();
 if(text.length>2048)return null;
 try{return JSON.parse(text)}catch{return null}
};
export async function startEmailLogin(request,env){
 if(!emailLoginReady(env))return fail('이메일 로그인 발송 설정이 준비되지 않았습니다.',503);
 const body=await parseJson(request);
 const email=cleanEmail(body?.email);
 if(!email)return fail('올바른 이메일 주소를 입력하세요.');
 const addressHash=await digest(email);
 const clientIp=request.headers.get('cf-connecting-ip')||'unknown';
 const ipHash=await digest(String(env.EMAIL_RATE_PEPPER||env.SESSION_SECRET||env.RESEND_API_KEY||'dominion-email-rate')+':'+clientIp);
 const since=epoch()-3600;
 const ipRate=await env.DB.prepare('SELECT COUNT(*) AS total FROM email_login_attempts WHERE ip_hash=? AND created_at>?').bind(ipHash,since).first();
 const mailRate=await env.DB.prepare('SELECT COUNT(*) AS total FROM email_login_attempts WHERE email_hash=? AND created_at>?').bind(addressHash,since).first();
 if(Number(ipRate?.total||0)>=20||Number(mailRate?.total||0)>=5)return fail('인증 메일 요청 횟수를 초과했습니다. 잠시 후 다시 시도하세요.',429);
 const id=random(), poll=random(), verify=random();
 const issued=epoch();
 await env.DB.prepare('INSERT INTO email_login_attempts(id,email,email_hash,ip_hash,poll_hash,verify_hash,status,created_at,expires_at) VALUES(?,?,?,?,?,?,?, ?,?)')
  .bind(id,email,addressHash,ipHash,await digest(poll),await digest(verify),'pending',issued,issued+TTL).run();
 const link=new URL('/v1/auth/email/verify',request.url);
 link.searchParams.set('id',id);
 link.searchParams.set('token',verify);
 const sender=String(env.EMAIL_FROM).trim();
 const message={
  from:sender,to:[email],subject:'DOMINION 로그인 이메일 확인',
  text:'DOMINION 로그인을 요청하셨다면 아래 링크에서 확인하세요. 링크는 10분 뒤 만료됩니다.\\n\\n'+link.href+'\\n\\n요청하지 않았다면 이 메일을 무시하세요.'
 };
 let sent=false;
 if(env.EMAIL?.send){
  // Cloudflare Email Service (2026); keep the Resend adapter for other plans.
  try{await env.EMAIL.send(message);sent=true}catch{}
 }else if(env.RESEND_API_KEY){
  const response=await fetch('https://api.resend.com/emails',{method:'POST',
   headers:{authorization:'Bearer '+env.RESEND_API_KEY,'content-type':'application/json'},
   body:JSON.stringify(message),signal:AbortSignal.timeout(12000)}).catch(()=>null);
  sent=Boolean(response?.ok);
 }
 if(!sent){
  await env.DB.prepare('DELETE FROM email_login_attempts WHERE id=?').bind(id).run();
  return fail('인증 메일을 발송하지 못했습니다. 잠시 후 다시 시도하세요.',502);
 }
 return reply({id,poll_token:poll,expires_in:TTL});
}
export async function verifyEmailLogin(request,env){
 if(!env.DB)return fail('인증 서버 연결 오류',503);
 const url=new URL(request.url);
 let id,secret;
 if(request.method==='GET'){
  id=url.searchParams.get('id');secret=url.searchParams.get('token');
 }else{
  if(Number(request.headers.get('content-length')||0)>2048)return fail('잘못된 요청');
  const body=await request.formData().catch(()=>null);
  id=body?.get('id');secret=body?.get('token');
 }
 if(!validSecret(id)||!validSecret(secret))return page('인증 링크 오류','로그인 링크가 올바르지 않습니다.');
 const row=await env.DB.prepare('SELECT verify_hash,status,expires_at FROM email_login_attempts WHERE id=?').bind(id).first();
 if(!row||row.expires_at<=epoch()||row.verify_hash!==await digest(secret))
  return page('인증 링크 만료','링크가 만료되었거나 올바르지 않습니다. DOMINION에서 다시 로그인하세요.');
 if(request.method==='GET'){
  // Link preview crawlers should not verify by merely visiting the URL.
  const action='<form method="post" action="/v1/auth/email/verify"><input type="hidden" name="id" value="'+id+'"><input type="hidden" name="token" value="'+secret+'"><button type="submit" style="padding:.8rem 1.4rem;font:inherit;cursor:pointer">이메일 주소 확인하기</button></form>';
  return page('DOMINION 이메일 확인','본인이 로그인을 요청했다면 아래 버튼으로 인증을 마치세요.',action);
 }
 if(row.status==='pending'){
  const changed=await env.DB.prepare("UPDATE email_login_attempts SET status='verified' WHERE id=? AND verify_hash=? AND status='pending' AND expires_at>?").bind(id,row.verify_hash,epoch()).run();
  if(changed.meta?.changes!==1)return page('인증 완료','이미 처리된 로그인 요청입니다.');
 }else if(row.status!=='verified'){
  return page('인증 완료','이미 처리된 로그인 요청입니다.');
 }
 return page('이메일 인증 완료','DOMINION 화면으로 돌아가세요. 로그인 상태가 자동으로 갱신됩니다.');
}
export async function pollEmailLogin(request,env){
 if(!env.DB)return fail('인증 서버 연결 오류',503);
 const body=await parseJson(request);
 const id=body?.id,secret=body?.poll_token;
 if(!validSecret(id)||!validSecret(secret))return fail('잘못된 로그인 확인 요청입니다.');
 const pollHash=await digest(secret);
 const row=await env.DB.prepare('SELECT status,expires_at FROM email_login_attempts WHERE id=? AND poll_hash=?').bind(id,pollHash).first();
 if(!row)return fail('로그인 요청을 찾을 수 없습니다.',404);
 if(row.expires_at<=epoch())return fail('이메일 로그인 링크가 만료됐습니다.',410);
 if(row.status==='pending')return reply({status:'waiting'});
 if(row.status!=='verified')return fail('이미 처리된 로그인 요청입니다.',409);
 const token=random(),tokenHash=await digest(token),expires=epoch()+7*86400;
 // D1 batch executes atomically; only the first valid claimant can insert a
 // session. A racing poll sees a consumed attempt and cannot mint a second one.
 const results=await env.DB.batch([
  env.DB.prepare("INSERT OR IGNORE INTO accounts(id,email,credits,created_at) SELECT 'email:' || email_hash,email,0,? FROM email_login_attempts WHERE id=? AND poll_hash=? AND status='verified' AND expires_at>?").bind(epoch(),id,pollHash,epoch()),
  env.DB.prepare("INSERT INTO sessions(token_hash,account_id,expires_at) SELECT ?,'email:' || email_hash,? FROM email_login_attempts WHERE id=? AND poll_hash=? AND status='verified' AND expires_at>?").bind(tokenHash,expires,id,pollHash,epoch()),
  env.DB.prepare("UPDATE email_login_attempts SET status='consumed' WHERE id=? AND poll_hash=? AND status='verified' AND expires_at>?").bind(id,pollHash,epoch())
 ]);
 if(results[1]?.meta?.changes!==1||results[2]?.meta?.changes!==1)return fail('이미 처리된 로그인 요청입니다.',409);
 return reply({status:'done',token,expires_in:7*86400});
}
export const emailAuthTest={cleanEmail,validSecret,digest};
