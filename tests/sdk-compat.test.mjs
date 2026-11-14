import test from 'node:test';
import assert from 'node:assert/strict';
import {selectCompatTools,buildCompatMessages} from '../server/tool-compat.mjs';

const baseline=[
 'workspace_list','workspace_read','workspace_write','file_list','file_read','file_write',
 'shell_exec','terminal_exec','web_search','http_fetch','account_status','github_api',
 'youtube_search','youtube_channels','youtube_upload','memory_store','memory_search','skill_propose'
];
const make=name=>({name,description:'Native '+name,
 parameters:{type:'object',properties:{task:{type:'string'}},required:['task']}});

test('SDK delegate appears for non-function-calling 4B model with many built-in tools',()=>{
 const tools=[...baseline.map(make),make('agent_delegate')];
 const offered=selectCompatTools(tools,['agent_delegate']);
 assert.equal(offered[0].name,'tool_discover');
 assert.ok(offered.some(x=>x.name==='agent_delegate'));
 const message=buildCompatMessages({
  system:'You must execute the task',history:[{role:'user',content:'Delegating a task'}],
  tools,extra:['agent_delegate']
 });
 assert.ok(message[0].content.includes('"name":"agent_delegate"'));
});

test('selected tools are bounded and SDK tool names are not duplicated',()=>{
 const tools=[...baseline.map(make),make('agent_delegate'),make('sdk_probe')];
 const names=selectCompatTools(tools,['sdk_probe','agent_delegate']).map(x=>x.name);
 assert.equal(names[1],'agent_delegate');
 assert.equal(names[2],'sdk_probe');
 assert.equal(new Set(names).size,names.length);
 assert.ok(names.length<=18);
});
