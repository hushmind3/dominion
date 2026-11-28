import {pendingApp} from './app_auth.js';
import {randomToken} from './token_crypto.js';
const escape=s=>String(s).replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
export async function connectPage(request,env){
 const id=new URL(request.url).searchParams.get('code');
 const pending=await pendingApp(env,id);
 const nonce=randomToken();
 const headers={'content-type':'text/html; charset=utf-8','cache-control':'no-store','referrer-policy':'no-referrer','x-content-type-options':'nosniff','cross-origin-opener-policy':'same-origin-allow-popups','content-security-policy':"default-src 'none'; base-uri 'none'; frame-ancestors 'none'; form-action 'none'; script-src 'nonce-"+nonce+"' https://accounts.google.com/gsi/client; style-src 'nonce-"+nonce+"' https://accounts.google.com/gsi/style; frame-src https://accounts.google.com/gsi/; connect-src 'self' https://accounts.google.com/gsi/"};
 const page=(content,script='')=>new Response('<!doctype html><html lang="ko"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>DOMINION Cloud</title><style nonce="'+nonce+'">body{font:16px system-ui;background:#f7f2ea;color:#292522;margin:0;padding:24px}main{max-width:480px;margin:10vh auto;background:white;border:1px solid #e7e0d5;border-radius:18px;padding:24px}button{padding:12px 16px;border:0;border-radius:10px;background:#b2573c;color:white;font:inherit}p{line-height:1.6}strong{overflow-wrap:anywhere}button:disabled{opacity:.5}</style><main><h1>DOMINION Cloud</h1>'+content+'</main>'+script+'</html>',{headers});
 if(!pending)return page('<p>연결 요청이 만료되었거나 이미 사용되었습니다. 앱에서 다시 시작하세요.</p>');
 const fallback='<p>Google 연결을 사용할 수 없다면 기존 DOMINION의 Cloud 계정 화면에서 이 연결 코드를 승인하세요.</p><p><strong>'+escape(id)+'</strong></p>';
 if(!env.GOOGLE_CLIENT_ID)return page('<p>웹 Google 인증 설정이 필요합니다.</p>'+fallback);
 // Pairing identifier also binds the signed Google ID token nonce. The app
 // bearer never reaches this page, and no owner session is issued here.
 const content='<p>연결 요청 앱: <strong>'+escape(pending.app_name)+'</strong></p><p>본인이 실행한 앱인지 확인하세요. 승인하면 동일 계정의 무료 모델 이용 한도를 7일간 공유합니다. 결제 및 계정 관리 권한은 제공하지 않습니다.</p><div id="google"></div><p id="status" role="status">Google 계정을 선택하세요.</p><button id="approve" disabled>이 앱 연결 승인</button>'+fallback;
 const script='<script nonce="'+nonce+'" src="https://accounts.google.com/gsi/client"></script><script nonce="'+nonce+'">const code='+JSON.stringify(id)+';const client='+JSON.stringify(env.GOOGLE_CLIENT_ID).replace(/</g,'\\u003c')+';let credential="";const status=document.getElementById("status"),approve=document.getElementById("approve");if(window.google){google.accounts.id.initialize({client_id:client,nonce:code,callback:r=>{credential=r.credential;approve.disabled=!credential;status.textContent="Google 계정 선택 완료. 앱 연결을 승인하세요."}});google.accounts.id.renderButton(document.getElementById("google"),{theme:"outline",size:"large"})}else{status.textContent="Google 로그인 창을 불러오지 못했습니다."}approve.onclick=async()=>{if(!credential)return;approve.disabled=true;try{const r=await fetch("/v1/apps/google/approve",{method:"POST",headers:{"content-type":"application/json"},body:JSON.stringify({id:code,credential})});const data=await r.json();if(!r.ok)throw Error(data.error||"인증 오류");credential="";status.textContent="연결 승인 완료. 앱으로 돌아가세요.";document.getElementById("google").hidden=true}catch(e){status.textContent=e.message;approve.disabled=false}};</script>';
 return page(content,script);
}
