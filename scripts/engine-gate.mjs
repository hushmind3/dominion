import {goals,goalEvents} from '../server/harness.mjs';
import {verifyRunTrace} from '../server/engine-contract.mjs';
const results=goals().map(run=>verifyRunTrace(run,goalEvents(run.id)));
const failed=results.filter(x=>!x.ok);
console.log(JSON.stringify({inspected:results.length,passed:results.length-failed.length,failed},null,2));
if(failed.length)process.exitCode=1;
