'use strict';
const $=id=>document.getElementById(id);
const csrf=document.querySelector('meta[name=voice-session]').content;
let busy=false,lastId='',pendingId='',lastAnswer='',audio=true,recognition=null;
async function api(path,body){
 const r=await fetch(path,{method:body===undefined?'GET':'POST',headers:{'X-Voice-Session':csrf,...(body===undefined?{}:{'Content-Type':'application/json'})},...(body===undefined?{}:{body:JSON.stringify(body)}),cache:'no-store'});
 const data=await r.json();if(!r.ok)throw Error(data.error||'연결 실패');return data;
}
function speak(text){
 if(!('speechSynthesis' in window)||!text)return;
 speechSynthesis.cancel();
 const u=new SpeechSynthesisUtterance(text.slice(0,12000));u.lang='ko-KR';speechSynthesis.speak(u);
}
function error(e){$('status').textContent=e.message||String(e)}
async function update(){
 try{
  const data=await api('/status');
  pendingId=data.pending?.requestId||'';
  $('status').textContent=pendingId?'도미니온 답변 대기 중 · 작업 연결에서 진행을 확인하세요.':'대화할 준비가 되었습니다.';
  const liveActive=!['stopped','failed'].includes(data.live.state);
  $('send').disabled=busy||!!pendingId||liveActive;
  $('discard').hidden=!pendingId;
  if(data.last){
   lastAnswer=data.last.answer;$('answer').textContent=lastAnswer;$('speak').disabled=false;
   if(lastId&&lastId!==data.last.requestId&&data.last.source==='browser'&&audio)speak(lastAnswer);
   lastId=data.last.requestId;
  }else if(!lastId){lastId='initial'}
  $('liveStatus').textContent=data.live.state+(data.live.error?' · '+data.live.error:'');
  $('liveStart').disabled=liveActive;
  $('managedStatus').textContent=$('liveStatus').textContent;
  ['pairStart','dial','listen'].forEach(id=>$(id).disabled=liveActive||busy);
  if(!liveActive)$('pairing').value='';
 }catch(e){error(e);$('send').disabled=true;}
}
$('send').onclick=async()=>{
 if(busy||pendingId)return;
 const text=$('utterance').value.trim();if(!text)return;
 busy=true;$('send').disabled=true;
 try{await api('/submit',{text});$('utterance').value='';await update()}catch(e){error(e)}finally{busy=false;$('send').disabled=!!pendingId}
};
$('mic').onclick=()=>{
 const Speech=window.SpeechRecognition||window.webkitSpeechRecognition;
 if(!Speech){error(Error('이 브라우저는 음성 인식을 지원하지 않습니다. 직접 입력하거나 LiveKit을 사용하세요.'));return}
 if(recognition){recognition.stop();return}
 if('speechSynthesis' in window)speechSynthesis.cancel();
 recognition=new Speech();recognition.lang='ko-KR';recognition.interimResults=false;
 recognition.onresult=e=>{$('utterance').value=e.results[0][0].transcript};
 recognition.onerror=e=>error(Error('음성 인식 오류: '+e.error));
 recognition.onend=()=>{recognition=null;$('mic').textContent='🎙 음성 입력'};
 try{recognition.start();$('mic').textContent='음성 입력 중 · 중지'}catch(e){recognition=null;error(e)}
};
$('mute').onclick=()=>{audio=!audio;if(!audio&&'speechSynthesis' in window)speechSynthesis.cancel();$('mute').textContent=audio?'음성 재생 끄기':'음성 재생 켜기'};
$('speak').onclick=()=>speak(lastAnswer);
$('discard').onclick=async()=>{if(!pendingId||!confirm('음성 화면의 대기 요청만 닫습니다. 이미 실행 중인 도미니온 작업은 별도로 취소해야 합니다.'))return;try{await api('/discard',{requestId:pendingId});await update()}catch(e){error(e)}};
$('liveStart').onclick=async()=>{
 if(!confirm('비공개 LiveKit 방으로 음성을 전송합니다. Deepgram·Cartesia 요금이 발생할 수 있습니다. 계속하시겠습니까?'))return;
 const config=Object.fromEntries(['url','room','token','sttKey','ttsKey','voice','language'].map(id=>[id,$(id).value.trim()]));
 try{await api('/live/start',config);['token','sttKey','ttsKey'].forEach(id=>$(id).value='');await update()}catch(e){$('liveStatus').textContent=e.message}
};
$('liveStop').onclick=async()=>{try{await api('/live/stop',{});$('liveStatus').textContent='중지 중'}catch(e){$('liveStatus').textContent=e.message}};
async function managed(mode){
 if(busy)return;
 if(mode==='outbound'&&!confirm('다음 번호로 AI 전화를 걸까요? 통화료가 발생할 수 있습니다.\n'+$('sipNumber').value.trim()))return;
 if(mode==='inbound'&&!confirm('전용 SIP 방에서 수신을 대기할까요? 허용 발신자와 통화 동의를 확인하세요.'))return;
 if(mode==='mobile'&&!confirm('휴대폰 전용 비공개 음성 방을 만들까요? 음성 공급자 요금이 발생할 수 있습니다.'))return;
 busy=true;['pairStart','dial','listen'].forEach(id=>$(id).disabled=true);
 const config=Object.fromEntries(['url','sttKey','ttsKey','voice','language'].map(id=>[id,$(id).value.trim()]));
 Object.assign(config,{mode,confirmed:true,apiKey:$('lkKey').value.trim(),apiSecret:$('lkSecret').value.trim(),trunk:$('sipTrunk').value.trim(),number:$('sipNumber').value.trim(),room:$('inboundRoom').value.trim(),dedicatedRoomConfirmed:$('inboundConfirm').checked});
 try{
  const r=await api('/live/managed',config);
  $('pairing').value=r.pairing?JSON.stringify(r.pairing):'';
  ['lkKey','lkSecret','sttKey','ttsKey'].forEach(id=>$(id).value='');
  $('managedStatus').textContent='연결 중…';
 }catch(e){$('managedStatus').textContent=e.message}
 finally{busy=false}
}
$('pairStart').onclick=()=>void managed('mobile');
$('dial').onclick=()=>void managed('outbound');
$('listen').onclick=()=>void managed('inbound');
$('pairCopy').onclick=async()=>{if(!$('pairing').value)return;try{await navigator.clipboard.writeText($('pairing').value);$('managedStatus').textContent='복사했습니다. 본인의 휴대폰에만 전달하세요.'}catch(e){$('pairing').select();$('managedStatus').textContent='선택된 정보를 직접 복사하세요.'}};
$('endSession').onclick=async()=>{try{await api('/live/stop',{});$('pairing').value='';$('managedStatus').textContent='종료 중…'}catch(e){$('managedStatus').textContent=e.message}};
void update();setInterval(update,1500);
