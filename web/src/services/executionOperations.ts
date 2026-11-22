/** Read-only, allowlisted projection of Sub2's native execution_operation receipts.
 * Never copy tool args, output, secrets, file content or opaque receipts into UI. */
export type ExecutionOperationSummary={
 id:string;tool?:string;kind?:string;state?:string;startedAt?:string;finishedAt?:string
}

function stringField(record:Record<string,unknown>,key:string):string|undefined{
 const value=record[key]
 return typeof value==='string'&&value.length>0?value:undefined
}
export function projectExecutionOperations(payload:unknown,runId:string):ExecutionOperationSummary[]{
 if(!Array.isArray(payload))throw new Error('Invalid execution operations response')
 return payload.flatMap((item:unknown):ExecutionOperationSummary[]=>{
  if(item===null||typeof item!=='object'||Array.isArray(item))return []
  const record=item as Record<string,unknown>
  if(record.runId!==runId)return []
  const receipt=record.toolReceipt&&typeof record.toolReceipt==='object'&&!Array.isArray(record.toolReceipt)
   ?record.toolReceipt as Record<string,unknown>:{}
  return [{
   id:stringField(record,'id')||'',
   tool:stringField(receipt,'tool')||stringField(record,'tool'),
   kind:stringField(record,'kind'),
   state:stringField(record,'effectState')||stringField(record,'status'),
   startedAt:stringField(record,'startedAt')||stringField(record,'createdAt'),
   finishedAt:stringField(record,'finishedAt')||stringField(record,'updatedAt'),
  }]
 })
}
