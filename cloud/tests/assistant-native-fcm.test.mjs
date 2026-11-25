import {test} from 'node:test';
import assert from 'node:assert/strict';
import {isPushToken,deliverPush} from '../src/assistant_push_delivery.js';

const fcm='fcm:'+'AbCd0123_:-'.repeat(12);
const expo='ExpoPushToken['+'a'.repeat(25)+']';
const env={
 ASSISTANT_PUSH_ENABLED:'1',ASSISTANT_FCM_PROJECT_ID:'my-native-project',
 ASSISTANT_FCM_SERVICE_EMAIL:'agent@my-native-project.iam.gserviceaccount.com',
 ASSISTANT_FCM_PRIVATE_KEY:'-----BEGIN PRIVATE KEY-----\nfake\n-----END PRIVATE KEY-----',
};
test('native and legacy token types are strictly distinct',()=>{
 assert.equal(isPushToken(fcm),true);
 assert.equal(isPushToken(expo),true);
 for(const token of ['not-a-token','fcm:short','fcm:evil@other','https://evil.test',
  'ExpoPushToken[x]'])assert.equal(isPushToken(token),false);
});
test('FCM HTTP v1 uses project-scoped OAuth, generic notification and no private text',async()=>{
 const calls=[];
 const send=async (url,options)=>{calls.push({url,options});
  return Response.json({name:'projects/my-native-project/messages/x'})};
 const status=await deliverPush(env,'google:owner',[fcm],'업무 결과를 확인하세요.','job',
  {send,access:async()=> 'fake-access-token'});
 assert.equal(status,'submitted');
 assert.equal(calls.length,1);
 assert.equal(calls[0].url,'https://fcm.googleapis.com/v1/projects/my-native-project/messages:send');
 assert.equal(calls[0].options.headers.authorization,'Bearer fake-access-token');
 const data=JSON.parse(calls[0].options.body).message;
 assert.equal(data.token,fcm.slice(4));
 assert.equal(data.notification.body,'업무 결과를 확인하세요.');
 assert.equal(data.data.kind,'job');
 assert.equal(data.data.assistantMessage,'true');
});
test('Expo users keep their existing provider and never receive FCM payloads',async()=>{
 let count=0;
 const send=async(url,options)=>{
  assert.equal(url,'https://exp.host/--/api/v2/push/send');
  assert.deepEqual(JSON.parse(options.body)[0].to,expo);count++;
  return Response.json({data:[{status:'ok'}]});
 };
 assert.equal(await deliverPush(env,'owner',[expo],'새 메시지','message',{send}),'submitted');
 assert.equal(count,1);
});
test('native provider absence cannot send to Expo or silently treat FCM as sent',async()=>{
 let calls=0;
 const send=async()=>{calls++;throw Error('unexpected')};
 const noFcm={ASSISTANT_PUSH_ENABLED:'1'};
 assert.equal(await deliverPush(noFcm,'owner',[fcm],'새 메시지','message',{send}),'not_configured');
 assert.equal(calls,0);
 assert.equal(await deliverPush({...noFcm,ASSISTANT_PUSH_ENABLED:'0'},'owner',[expo],'새 메시지','message',{send}),'disabled');
});
test('stale FCM registration removal only on UNREGISTERED',async()=>{
 const deletes=[];
 const DB={prepare(sql){return {bind(token,account){return {run:async()=>deletes.push({token,account,sql})}}}}};
 const send=async()=>Response.json({error:{details:[{errorCode:'UNREGISTERED'}]}},{status:404});
 const state=await deliverPush({...env,DB},'owner',[fcm],'알림','job',
  {send,access:async()=> 'fake-access-token'});
 assert.equal(state,'failed');
 assert.equal(deletes.length,1);assert.equal(deletes[0].token,fcm);
});
