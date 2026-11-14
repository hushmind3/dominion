import {experiments,experiment,createExperiment,acceptExperiment,cancelExperiment,nextExperiment} from '../rsi-engine.mjs';
import {enqueue} from '../jobs.mjs';

// Called only AFTER the gateway has enforced its admin-token/origin policy.
export async function handleRsiRoutes({url,method,res,json,body,req}){
 const send=(data,status=200)=>{json(res,data,status);return true};
 if(url==='/api/rsi/experiments'){
  if(method==='GET')return send(experiments());
  if(method==='POST'){
   const row=createExperiment(await body(req));
   const job=enqueue('rsi',{experimentId:row.id});
   return send({...row,jobId:job.id},201);
  }
 }
 const idMatch=url.match(/^\/api\/rsi\/experiments\/([^/]+)$/);
 if(idMatch&&method==='GET'){
  const row=experiment(idMatch[1]);
  return send(row||{error:'Experiment not found'},row?200:404);
 }
 const action=url.match(/^\/api\/rsi\/experiments\/([^/]+)\/(accept|cancel|next)$/);
 if(action&&method==='POST'){
  const [,id,kind]=action;
  if(kind==='accept')return send(await acceptExperiment(id));
  if(kind==='cancel')return send(cancelExperiment(id));
  const row=nextExperiment(id,await body(req));
  const job=enqueue('rsi',{experimentId:row.id});
  return send({...row,jobId:job.id},201);
 }
 return false;
}
