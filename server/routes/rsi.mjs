import {experiments,experiment,createExperiment,acceptExperiment,cancelExperiment,nextExperiment} from '../rsi-engine.mjs';
import {enqueue} from '../jobs.mjs';

// Called only AFTER the gateway has enforced its admin-token/origin policy.
export async function handleRsiRoutes({url,method,res,json,body,req}){
 if(url==='/api/rsi/experiments'){
  if(method==='GET')return json(res,experiments());
  if(method==='POST'){
   const row=createExperiment(await body(req));
   const job=enqueue('rsi',{experimentId:row.id});
   return json(res,{...row,jobId:job.id},201);
  }
 }
 const idMatch=url.match(/^\/api\/rsi\/experiments\/([^/]+)$/);
 if(idMatch&&method==='GET'){
  const row=experiment(idMatch[1]);
  return json(res,row||{error:'Experiment not found'},row?200:404);
 }
 const action=url.match(/^\/api\/rsi\/experiments\/([^/]+)\/(accept|cancel|next)$/);
 if(action&&method==='POST'){
  const [,id,kind]=action;
  if(kind==='accept')return json(res,await acceptExperiment(id));
  if(kind==='cancel')return json(res,cancelExperiment(id));
  const row=nextExperiment(id,await body(req));
  const job=enqueue('rsi',{experimentId:row.id});
  return json(res,{...row,jobId:job.id},201);
 }
 return false;
}
