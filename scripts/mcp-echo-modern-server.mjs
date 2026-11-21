#!/usr/bin/env node
// Test-only modern MCP 2026-07-28 stdio fixture (no initialize handshake).
import readline from 'node:readline'
import {appendFileSync} from 'node:fs'
const log=process.env.DOMINION_MCP_TEST_MODERN_LOG
if(log)appendFileSync(log,'started\n')
const send=message=>process.stdout.write(JSON.stringify(message)+'\n')
const rl=readline.createInterface({input:process.stdin})
rl.on('line',line=>{
  let request
  try{request=JSON.parse(line)}catch{return}
  const metadata=request.params?._meta||{}
  const valid=metadata['io.modelcontextprotocol/protocolVersion']==='2026-07-28'
    &&metadata['io.modelcontextprotocol/clientCapabilities']
    &&metadata['io.modelcontextprotocol/clientInfo']?.name==='dominion-rust'
  if(!valid){
    if(log)appendFileSync(log,'invalid-or-initialize\n')
    send({jsonrpc:'2.0',id:request.id,error:{code:-32020,message:'missing modern request metadata'}})
    return
  }
  let result
  switch(request.method){
    case 'server/discover':
      result={resultType:'complete',supportedVersions:['2026-07-28'],
        capabilities:{tools:{}}}
      break
    case 'tools/list':
      result={resultType:'complete',tools:[{name:'echo',
        description:'Modern stdio echo',
        inputSchema:{type:'object',properties:{text:{type:'string'}},required:['text']}}]}
      break
    case 'tools/call':
      if(request.params?.name!=='echo'){
        send({jsonrpc:'2.0',id:request.id,error:{code:-32601,message:'Unknown tool'}})
        return
      }
      result={resultType:'complete',content:[{type:'text',
        text:'MCP_MODERN: '+String(request.params?.arguments?.text??'')}]}
      break
    default:
      send({jsonrpc:'2.0',id:request.id,error:{code:-32601,message:'Method not found'}})
      return
  }
  send({jsonrpc:'2.0',id:request.id,result})
})
