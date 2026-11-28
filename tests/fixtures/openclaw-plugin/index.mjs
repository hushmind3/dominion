// Executes only inside the test's private OpenClaw profile; no network/model calls.
let running=false,hookCalls=0
export default {
 id:'dominion-host-fixture',name:'Dominion host fixture',
 register(api){
  api.registerService({id:'dominion-fixture-service',start(){running=true},stop(){running=false}})
  api.on('before_tool_call',()=>{hookCalls++})
  api.registerTool({name:'dominion_fixture_echo',description:'Return a test value and actual service/hook state.',parameters:{type:'object',properties:{text:{type:'string'}},required:['text'],additionalProperties:false},async execute(_id,args){return {content:[{type:'text',text:JSON.stringify({text:(api.pluginConfig?.prefix||'')+args.text,running,hookCalls})}]}}})
  api.registerGatewayMethod('dominion.fixture.status',({respond})=>respond(true,{running,hookCalls}))
 }
}
