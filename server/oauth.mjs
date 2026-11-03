import {randomBytes} from 'node:crypto';
import {put,all,log} from './store.mjs';
import {secretSet,secretGet} from './vault.mjs';
const stateCache=new Map();
const appDefs={
 github:{authorize:'https://github.com/login/oauth/authorize',token:'https://github.com/login/oauth/access_token',scope:'read:user repo',env:['GITHUB_CLIENT_ID','GITHUB_CLIENT_SECRET']},
 google:{authorize:'https://accounts.google.com/o/oauth2/v2/auth',token:'https://oauth2.googleapis.com/token',scope:'openid profile email https://www.googleapis.com/auth/youtube.readonly',env:['GOOGLE_CLIENT_ID','GOOGLE_CLIENT_SECRET']}
};
function appConfig(provider){
 const def=appDefs[provider];if(!def)throw Error('Unknown OAuth provider');
 const id=process.env[def.env[0]]||secretGet('oauth_app:'+provider+':id')||'';
 const secret=process.env[def.env[1]]||secretGet('oauth_app:'+provider+':secret')||'';
 return {...def,id,secret};
}
export const oauthCallbackBase=()=>process.env.DOMINION_OAUTH_ORIGIN||'http://127.0.0.1:53682';
export const oauthProviders=()=>Object.keys(appDefs).map(id=>{const app=appConfig(id);return {id,configured:Boolean(app.id&&app.secret),scope:app.scope,callback:oauthCallbackBase()+'/api/oauth/'+id+'/callback'}});
export function saveOAuthApp(provider,{clientId='',clientSecret=''}={}){
 const def=appDefs[provider];if(!def)throw Error('Unknown OAuth provider');
 const id=String(clientId||'').trim(),secret=String(clientSecret||'').trim();
 if(!id||!secret)throw Error('Client ID and Client Secret are required');
 secretSet('oauth_app:'+provider+':id',id);
 secretSet('oauth_app:'+provider+':secret',secret);
 log('oauth.app_saved',{provider});
 return oauthProviders().find(item=>item.id===provider);
}
export const connections=()=>all('connection').map(x=>({...x,hasToken:Boolean(secretGet('oauth:'+x.id))}));
export function oauthStart(provider,origin){
 const app=appConfig(provider);if(!app?.id||!app?.secret)throw Error('Configure OAuth client ID and secret first');
 const state=randomBytes(24).toString('hex');
 const redirect=origin+'/api/oauth/'+provider+'/callback';
 stateCache.set(state,{provider,redirect,at:Date.now()});
 const url=new URL(app.authorize);
 url.searchParams.set('client_id',app.id);url.searchParams.set('redirect_uri',redirect);url.searchParams.set('scope',app.scope);url.searchParams.set('state',state);
 if(provider==='google'){url.searchParams.set('response_type','code');url.searchParams.set('access_type','offline');url.searchParams.set('prompt','consent');}
 return url.toString();
}
export async function oauthCallback(provider,code,state){
 const item=stateCache.get(state);stateCache.delete(state);
 if(!item||item.provider!==provider||Date.now()-item.at>600000)throw Error('OAuth state expired or invalid');
 const app=appConfig(provider);const payload={client_id:app.id,client_secret:app.secret,code,redirect_uri:item.redirect,grant_type:'authorization_code'};
 const r=await fetch(app.token,{method:'POST',headers:provider==='github'?{'Accept':'application/json','Content-Type':'application/json'}:{'Content-Type':'application/x-www-form-urlencoded'},body:provider==='github'?JSON.stringify(payload):new URLSearchParams(payload),signal:AbortSignal.timeout(12000)});
 if(!r.ok)throw Error('OAuth token exchange '+r.status);
 const data=await r.json();if(!data.access_token)throw Error(data.error_description||data.error||'No OAuth token received');
 let profile={};
 try{
  const url=provider==='github'?'https://api.github.com/user':'https://openidconnect.googleapis.com/v1/userinfo';
  const p=await fetch(url,{headers:{Authorization:'Bearer '+data.access_token,'User-Agent':'DOMINION'},signal:AbortSignal.timeout(10000)});if(p.ok)profile=await p.json();
 }catch{}
 const id=provider+':'+String(profile.id||profile.sub||crypto.randomUUID());
 secretSet('oauth:'+id,data.access_token);
 if(data.refresh_token)secretSet('oauth_refresh:'+id,data.refresh_token);
 const connection=put('connection',{id,provider,displayName:profile.login||profile.name||profile.email||id,scopes:app.scope,expiresAt:data.expires_in?Date.now()+data.expires_in*1000:null});
 log('oauth.connected',{provider,id});return connection;
}
export async function connectorCall(provider,action,args={}){
 const account=connections().find(x=>x.provider===provider);if(!account)throw Error('Connect '+provider+' first');
 const token=secretGet('oauth:'+account.id);if(!token)throw Error('Reconnect '+provider);
 let url;
 if(provider==='github'){
  if(action==='repos')url='https://api.github.com/user/repos?per_page=40&sort=updated';
  else if(action==='me')url='https://api.github.com/user';
  else throw Error('Unknown GitHub action');
 }else if(provider==='google'){
  if(action==='youtube_search'){url=new URL('https://www.googleapis.com/youtube/v3/search');url.searchParams.set('part','snippet');url.searchParams.set('type','video');url.searchParams.set('maxResults',String(Math.min(50,Math.max(1,Number(args.maxResults)||25))));if(args.q)url.searchParams.set('q',String(args.q));else url.searchParams.set('forMine','true');}
  else if(action==='youtube_channels')url='https://www.googleapis.com/youtube/v3/channels?part=snippet,statistics&mine=true';
  else throw Error('Unknown Google action');
 }else throw Error('Unsupported provider');
 const r=await fetch(url,{headers:{Authorization:'Bearer '+token,'User-Agent':'DOMINION','Accept':'application/json'},signal:AbortSignal.timeout(15000)});
 if(!r.ok)throw Error(provider+' API '+r.status+': '+(await r.text()).slice(0,500));
 return r.json();
}
export async function githubRequest(method,path,body){
 const account=connections().find(x=>x.provider==='github');if(!account)throw Error('Connect a GitHub account first');
 const token=secretGet('oauth:'+account.id);if(!token)throw Error('Reconnect GitHub');
 const route=String(path||'');if(!route.startsWith('/')||route.startsWith('//')||route.includes('..'))throw Error('GitHub path must start with / and cannot contain ..');
 const verb=String(method||'GET').toUpperCase();if(!['GET','POST','PATCH','PUT','DELETE'].includes(verb))throw Error('Unsupported GitHub method');
 const response=await fetch('https://api.github.com'+route,{method:verb,headers:{Authorization:'Bearer '+token,Accept:'application/vnd.github+json','User-Agent':'DOMINION','X-GitHub-Api-Version':'2022-11-28','Content-Type':'application/json'},body:body===undefined?undefined:JSON.stringify(body),signal:AbortSignal.timeout(20000)});
 const text=await response.text();
 if(!response.ok)throw Error('GitHub API '+response.status+': '+text.slice(0,700));
 if(!text)return {status:response.status};
 try{return JSON.parse(text)}catch{return {status:response.status,text:text.slice(0,4000)}}
}
