// DOMINION native batch scheduler. Independent observation calls may run together.
// Writes and unknown/external effects are ordered barriers, never speculative parallel mutations.
// Design reference: OpenAI Agents SDK toolExecution.ts (bounded workers / drain on error).
const readOnly=new Set([
  'workspace_list','workspace_read','file_list','file_read',
  'web_search','http_fetch','account_status','youtube_search','youtube_channels',
  'memory_search','dev_status','dev_list','dev_read','dev_diff','tool_discover'
]);

export function isReadOnlyCall(call){
  const name=String(call?.function?.name||'');
  if(readOnly.has(name))return true;
  if(name==='github_api'){
    try{
      const args=JSON.parse(call.function.arguments||'{}');
      return String(args.method||'').toUpperCase()==='GET';
    }catch{return false;}
  }
  // MCP and unknown tools can mutate even when named "read": execute serially.
  return false;
}
const abort=signal=>signal?.throwIfAborted?.();
export async function executeToolBatch(calls,execute,{signal,maxReadConcurrency=4}={}){
  if(!Array.isArray(calls))throw Error('Tool batch must be an array');
  if(typeof execute!=='function')throw Error('Tool batch executor required');
  if(!Number.isInteger(maxReadConcurrency)||maxReadConcurrency<1||maxReadConcurrency>16)
    throw Error('Invalid read concurrency (1-16)');
  const results=Array(calls.length);
  let pos=0;
  while(pos<calls.length){
    abort(signal);
    if(!isReadOnlyCall(calls[pos])){
      // Mutations form a barrier. The preceding reads have fully settled.
      results[pos]=await execute(calls[pos],pos);
      pos++;
      continue;
    }
    const begin=pos;
    while(pos<calls.length&&isReadOnlyCall(calls[pos]))pos++;
    let next=begin,firstError;
    const worker=async()=>{
      while(next<pos&&!firstError&&!signal?.aborted){
        const index=next++;
        try{results[index]=await execute(calls[index],index);}
        catch(error){firstError??=error;}
      }
    };
    // Every started worker is drained, including when one rejects or the run is aborted.
    // A cancelled remote operation can already have had effects; never claim rollback.
    await Promise.allSettled(
      Array.from({length:Math.min(maxReadConcurrency,pos-begin)},()=>worker())
    );
    if(firstError)throw firstError;
    abort(signal);
  }
  return results;
}
