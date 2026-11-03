import {verifyRunTrace} from './engine-contract.mjs';
export function inspectTraces(runs,readEvents){
 const items=runs.map(run=>verifyRunTrace(run,readEvents(run.id)));
 const failed=items.filter(x=>!x.ok);
 if(!items.length)failed.push({errors:['No trace records inspected']});
 return {inspected:items.length,passed:items.length-failed.length,failed,ok:failed.length===0};
}
