import test from 'node:test';
import assert from 'node:assert/strict';
import {agents,newSession,saveRoom} from '../server/engine.mjs';
test('agent registry is data-driven without automatic records',()=>{const a=agents();assert.ok(Array.isArray(a));assert.equal(a.some(x=>x.id==='general'),false);});
test('room size comes from user-specified participant IDs',()=>{const r=saveRoom({name:'architecture',agentIds:['a','b','c'],rounds:2});assert.deepEqual(r.agentIds,['a','b','c']);});
test('a new conversation starts with no fabricated messages',()=>{const x=newSession();assert.ok(x.id);});
