// Deterministic same-task, same-model evidence comparison. Results are not a
// benchmark unless tool, budget and model metadata match and outcomes have
// independent verifiers. Only read-only JSON snapshots are supported.
import {readFileSync} from 'node:fs';
import {pathToFileURL} from 'node:url';
export function compare(base,candidate){
 const a=base.tasks,b=candidate.tasks;
 if(!Array.isArray(a)||!Array.isArray(b))throw Error('Both snapshots need native task arrays');
 const toMap=items=>new Map(items.map(x=>[x.taskId,x]));
 const before=toMap(a),after=toMap(b);
 if(before.size!==a.length||after.size!==b.length)throw Error('Duplicate task identifiers');
 const ids=[...before.keys()].sort();
 if(ids.length!==after.size||ids.some(id=>!after.has(id)))
   throw Error('Candidate task IDs must exactly match the baseline cohort');
 const conditions=[];
 const metaA=base.comparisonMeta||{},metaB=candidate.comparisonMeta||{};
 for(const key of ['toolSchemaHash','maxRounds','maxTools','providerPolicy']){
   if(metaA[key]===undefined||metaB[key]===undefined)conditions.push(key+' missing');
   else if(metaA[key]!==metaB[key])conditions.push(key+' mismatch');
 }
 const sample=[];
 for(const id of ids){
   const left=before.get(id),right=after.get(id);
   if(left.verified!==true||right.verified!==true){
     conditions.push(id+': independent verifier missing');continue;
   }
   if(left.model!==right.model||left.providerId!==right.providerId){
     conditions.push(id+': model/provider mismatch');continue;
   }
   if(!['succeeded','failed'].includes(left.taskOutcome)||!['succeeded','failed'].includes(right.taskOutcome)){
     conditions.push(id+': missing terminal result');continue;
   }
   sample.push({taskId:id,before:left,after:right});
 }
 const aggregate=key=>({
   pass:sample.filter(t=>t[key].taskOutcome==='succeeded').length,
   tokens:sample.reduce((n,t)=>n+(Number.isFinite(t[key].tokens)?t[key].tokens:0),0),
   modelCalls:sample.reduce((n,t)=>n+(Number.isFinite(t[key].modelCalls)?t[key].modelCalls:0),0),
   elapsedMs:sample.reduce((n,t)=>n+(Number.isFinite(t[key].durationMs)?t[key].durationMs:0),0),
   costUsd:null, // snapshots currently lack per-execution invoice attribution
 });
 const baseline=aggregate('before'),current=aggregate('after');
 return {
   comparable:conditions.length===0&&sample.length>0,
   verification:'native_independent_result_verifier',
   requestedTasks:ids.length,pairedVerified:sample.length,
   conditionsMissing:conditions,
   baseline:{...baseline,successRate:sample.length?baseline.pass/sample.length:null},
   candidate:{...current,successRate:sample.length?current.pass/sample.length:null},
   delta:sample.length&&conditions.length===0?{
     passed:current.pass-baseline.pass,
     tokens:current.tokens-baseline.tokens,
     durationMs:current.elapsedMs-baseline.elapsedMs,
   }:null,
   costClaim:'unavailable_without_per_execution_provider_invoice_and_gpu_receipts'
 };
}
if(process.argv[1]&&import.meta.url===pathToFileURL(process.argv[1]).href){
 const [a,b]=process.argv.slice(2);
 if(!a||!b){console.error('Usage: node scripts/sub2-compare.mjs baseline.json candidate.json');process.exit(2);}
 try{
   const result=compare(JSON.parse(readFileSync(a,'utf8')),JSON.parse(readFileSync(b,'utf8')));
   process.stdout.write(JSON.stringify(result,null,2)+'\n');
   if(!result.comparable)process.exitCode=1;
 }catch(error){console.error(String(error));process.exitCode=2;}
}
