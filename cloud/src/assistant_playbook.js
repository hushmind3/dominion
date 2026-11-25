// Verified browser experience only: hints for planning, never executable macros.
const ACTIONS=new Set(['navigate','click','select','fill','download_pdf','save_pdf']);
const SENSITIVE=/(?:비밀번호|인증번호|주민번호|주민등록|계좌|카드번호|휴대폰|전화번호|생년월일|password|passcode|otp|token|secret|api.?key|credit.?card|iban|social.?security)/i;
const normalize=s=>typeof s==='string'?s.trim().replace(/\s+/g,' ').toLocaleLowerCase('ko-KR'):'';
const safeLabel=s=>{
 if(typeof s!=='string'||!s.trim()||SENSITIVE.test(s)||/\b\d{7,}\b/.test(s)||/@/.test(s))return '';
 return s.replace(/[\r\n\t]/g,' ').replace(/\b(?:\d[ -]?){6,}\b/g,'[masked]').trim().slice(0,100);
};
export async function playbookKey(site,goal){
 const raw=normalize(site)+'\u0000'+normalize(goal);
 if(!site||!normalize(goal)||raw.length>4600)throw Error('Invalid playbook key');
 const hash=await crypto.subtle.digest('SHA-256',new TextEncoder().encode(raw));
 return 'playbook:'+Array.from(new Uint8Array(hash),x=>x.toString(16).padStart(2,'0')).join('').slice(0,40);
}
export function extractPlaybook(job,key,at=Date.now()){
 if(!job||job.status!=='completed'||!job.result?.evidence||!Array.isArray(job.trace)||job.trace.length===0||typeof job.root!=='string'||!normalize(job.goal)||typeof key!=='string'||!/^playbook:[a-f0-9]{40}$/.test(key))return null;
 const steps=job.trace.slice(-8).flatMap(row=>{
  if(!row||!ACTIONS.has(row.action))return [];
  const label=safeLabel(row.label);
  if(!label)return [];
  let page='';
  try{const u=new URL(row.page);if(u.protocol!=='https:'||u.hostname!==job.root&&!u.hostname.endsWith('.'+job.root))return [];page=u.origin+u.pathname.slice(0,140)}catch{return []}
  return [{action:row.action,label,page}];
 });
 if(!steps.length)return null;
 return {version:1,account:job.account,site:job.root,key,steps,createdAt:at,expiresAt:at+30*86400_000};
}
export function playbookHints(record,{account,site,key,at=Date.now()}={}){
 if(!record||record.version!==1||record.account!==account||record.site!==site||record.key!==key||!Number.isFinite(record.expiresAt)||at>=record.expiresAt||!Array.isArray(record.steps))return [];
 return record.steps.slice(-8).flatMap(row=>row&&ACTIONS.has(row.action)&&safeLabel(row.label)?[{action:row.action,label:safeLabel(row.label),page:typeof row.page==='string'?row.page.slice(0,180):''}]:[]);
}
