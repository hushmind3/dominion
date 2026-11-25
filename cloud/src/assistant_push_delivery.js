// Provider-scoped push transport. Durable inbox/job status is authoritative.
// Expo token format remains unchanged; native FCM tokens use an explicit prefix.
const expo=/^(?:ExpoPushToken|ExponentPushToken)\[[A-Za-z0-9_-]{10,200}\]$/;
const native=/^fcm:[A-Za-z0-9_:\-]{40,400}$/;
export const isPushToken=t=>typeof t==='string'&&(expo.test(t)||native.test(t));
const toBase64Url=bytes=>btoa(String.fromCharCode(...bytes)).replace(/\+/g,'-').replace(/\//g,'_').replace(/=+$/,'');
const encoded=s=>toBase64Url(new TextEncoder().encode(s));
const fcmConfig=env=>typeof env.ASSISTANT_FCM_PROJECT_ID==='string'&&
 /^[a-z][a-z0-9-]{3,62}$/.test(env.ASSISTANT_FCM_PROJECT_ID)&&
 typeof env.ASSISTANT_FCM_SERVICE_EMAIL==='string'&&
 /^[A-Za-z0-9._-]+@[A-Za-z0-9.-]+\.gserviceaccount\.com$/.test(env.ASSISTANT_FCM_SERVICE_EMAIL)&&
 typeof env.ASSISTANT_FCM_PRIVATE_KEY==='string'&&
 env.ASSISTANT_FCM_PRIVATE_KEY.includes('-----BEGIN PRIVATE KEY-----');
export async function serviceAccess(env,send=fetch,clock=()=>Date.now()){
 if(!fcmConfig(env))throw Error('FCM not configured');
 const base=env.ASSISTANT_FCM_PRIVATE_KEY.trim()
   .replace('-----BEGIN PRIVATE KEY-----','')
   .replace('-----END PRIVATE KEY-----','').replace(/\s/g,'');
 const raw=Uint8Array.from(atob(base),c=>c.charCodeAt(0));
 const key=await crypto.subtle.importKey('pkcs8',raw,{name:'RSASSA-PKCS1-v1_5',hash:'SHA-256'},false,['sign']);
 const now=Math.floor(clock()/1000);
 const header=encoded(JSON.stringify({alg:'RS256',typ:'JWT'}));
 const payload=encoded(JSON.stringify({iss:env.ASSISTANT_FCM_SERVICE_EMAIL,
   scope:'https://www.googleapis.com/auth/firebase.messaging',
   aud:'https://oauth2.googleapis.com/token',iat:now,exp:now+3500}));
 const content=header+'.'+payload;
 const signature=toBase64Url(new Uint8Array(
   await crypto.subtle.sign({name:'RSASSA-PKCS1-v1_5'},key,new TextEncoder().encode(content))));
 const response=await send('https://oauth2.googleapis.com/token',{
  method:'POST',headers:{'content-type':'application/x-www-form-urlencoded'},
  body:new URLSearchParams({grant_type:'urn:ietf:params:oauth:grant-type:jwt-bearer',
    assertion:content+'.'+signature}),signal:AbortSignal.timeout(10000)
 });
 if(!response.ok)throw Error('FCM service authorization unavailable');
 const body=await response.json();
 if(typeof body.access_token!=='string'||body.access_token.length<20||!Number.isFinite(body.expires_in))
  throw Error('Invalid FCM access response');
 return body.access_token;
}
export async function deliverPush(env,account,tokens,body,kind,{send=fetch,access=serviceAccess}={}){
 if(env.ASSISTANT_PUSH_ENABLED!=='1')return 'disabled';
 const eligible=[...new Set(tokens)].filter(isPushToken).slice(0,10);
 if(!eligible.length)return 'unregistered';
 let ok=false,configured=false;
 const legacy=eligible.filter(t=>expo.test(t));
 if(legacy.length){
  configured=true;
  try {
   const r=await send('https://exp.host/--/api/v2/push/send',{
    method:'POST',headers:{'content-type':'application/json',
     ...(env.EXPO_PUSH_ACCESS_TOKEN?{authorization:'Bearer '+env.EXPO_PUSH_ACCESS_TOKEN}:{})},
    body:JSON.stringify(legacy.map(to=>({to,title:'나의 AI 비서',body,
     sound:'default',channelId:'assistant-reminders',
     data:{assistantMessage:true,kind}}))),
    signal:AbortSignal.timeout(10000)
   });
   if(r.ok){
    const response=await r.json();const records=Array.isArray(response.data)?response.data:[response.data];
    for(let i=0;i<legacy.length;i++){
     if(records[i]?.status==='ok')ok=true;
     if(records[i]?.details?.error==='DeviceNotRegistered')
      await env.DB.prepare('DELETE FROM assistant_push WHERE token=? AND account_id=?').bind(legacy[i],account).run();
    }
   }
  }catch{/* best effort */}
 }
 const fcm=eligible.filter(t=>native.test(t));
 if(fcm.length&&fcmConfig(env)){
  configured=true;
  try{
   const token=await access(env,send);
   for(const device of fcm){
    try{
     const response=await send(
      'https://fcm.googleapis.com/v1/projects/'+env.ASSISTANT_FCM_PROJECT_ID+'/messages:send',{
      method:'POST',headers:{authorization:'Bearer '+token,'content-type':'application/json'},
      body:JSON.stringify({message:{token:device.slice(4),
       notification:{title:'나의 AI 비서',body},
       data:{assistantMessage:'true',kind},
       android:{priority:'normal',notification:{channel_id:'dominion_assistant_alerts'}}}}),
      signal:AbortSignal.timeout(10000)
     });
     if(response.ok)ok=true;
     else if(response.status===404){
      const problem=await response.json().catch(()=>null);
      if(problem?.error?.details?.some(x=>x.errorCode==='UNREGISTERED')){
       await env.DB.prepare('DELETE FROM assistant_push WHERE token=? AND account_id=?').bind(device,account).run();
      }
     }
    }catch{/* continue other subscribers */}
   }
  }catch{/* no credential/token means no FCM delivery, inbox remains durable */}
 }
 return ok?'submitted':configured?'failed':'not_configured';
}
