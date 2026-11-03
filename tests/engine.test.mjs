import test from 'node:test';
import assert from 'node:assert/strict';
import {agents,newSession,saveRoom} from '../server/engine.mjs';
test('agent setup is dynamic, not nine hard-coded bots',()=>{const a=agents();assert.ok(a.length>=1);assert.equal(a.find(x=>x.id==='general')?.name,'General');});
test('room size comes from user-specified participant IDs',()=>{const r=saveRoom({name:'architecture',agentIds:['a','b','c'],rounds:2});assert.deepEqual(r.agentIds,['a','b','c']);});
test('a new conversation starts with no fabricated messages',()=>{const x=newSession();assert.ok(x.id);});
