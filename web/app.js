const byId = (id) => document.getElementById(id);
const ui = {
  prompt: byId('prompt'), form: byId('submitForm'), run: byId('run'),
  write: byId('write'), shell: byId('shell'), mcp: byId('mcp'), evidence: byId('evidence'),
  maxSteps: byId('maxSteps'), tokenBudget: byId('tokenBudget'),
  planningMode: byId('planningMode'),
  sessionId: byId('sessionId'), notice: byId('notice'), work: byId('work'),
  status: byId('runStatus'), output: byId('output'), timeline: byId('timeline'),
  steps: byId('steps'), tokens: byId('tokens'), verified: byId('verified'),
  cancel: byId('cancel'), steer: byId('steer'), instruction: byId('instruction'),
  tasks: byId('tasks'), refresh: byId('refresh'), connection: byId('connection'),
  engineLed: byId('engineLed')
};
let active = null;
let stream = null;
let pending = null;

function notice(message) {
  ui.notice.textContent = message;
  ui.notice.hidden = !message;
}
function terminal(status) {
  return ['completed','failed','cancelled','needs_review'].includes(status);
}
async function api(path, method='GET', body) {
  const init = {method, headers:{'Accept':'application/json'}};
  if (body !== undefined) {
    init.headers['Content-Type']='application/json';
    init.body=JSON.stringify(body);
  }
  const response=await fetch(path,init);
  const data=await response.json();
  if (!response.ok) throw new Error(data.error || 'HTTP '+response.status);
  return data;
}
function row(label,detail) {
  const item=document.createElement('li');
  item.textContent=label+' · '+detail;
  ui.timeline.append(item);
  if (ui.timeline.children.length>150) ui.timeline.firstChild.remove();
  ui.timeline.scrollTop=ui.timeline.scrollHeight;
}
function statusText(status) {
  const labels={queued:'대기',running:'실행 중',completed:'완료',
    failed:'실패',cancelled:'중단됨',needs_review:'검토 필요'};
  return labels[status] || status;
}
function renderTask(record) {
  if (!record) return;
  active=record.id;
  ui.work.hidden=false;
  ui.status.textContent=statusText(record.status);
  ui.steps.textContent=String(record.steps || 0);
  ui.tokens.textContent=Number(record.tokens||0).toLocaleString('ko-KR');
  ui.verified.textContent=String(record.result?.verifiedWrites ?? 0);
  if (record.result?.answer) ui.output.textContent=record.result.answer;
  else if (record.error) ui.output.textContent=record.error;
  else if (record.status==='queued') ui.output.textContent='모델 실행을 준비하고 있습니다.';
  ui.cancel.disabled=terminal(record.status);
  ui.steer.disabled=terminal(record.status);
}
async function updateTask() {
  if (!active) return;
  const task=await api('/api/tasks/'+encodeURIComponent(active));
  renderTask(task);
  return task;
}
function openStream(id) {
  if (stream) stream.close();
  ui.timeline.replaceChildren();
  stream=new EventSource('/api/tasks/'+encodeURIComponent(id)+'/events?after=0');
  stream.onmessage=async (message) => {
    try {
      const event=JSON.parse(message.data);
      if (event.taskId!==active) return;
      const detail=event.detail || {};
      if (event.type==='tool_result') {
        row('도구 결과',detail.name+' · '+(detail.outcome?.ok ? '실행 기록됨' : '실패'));
      } else if (event.type==='plan') {
        row('실행 계획',String(detail.text||'계획 저장됨').slice(0,180));
      } else if (event.type==='verification') {
        row('결과 검증',detail.complete ? '검증 통과' : (detail.feedback||'보완 필요'));
      } else if (event.type==='context_compacted') {
        row('문맥 관리',String(detail.oldToolResults||0)+'개 오래된 도구 결과 압축');
      } else if (event.type==='model_step') {
        row('모델 단계',String(detail.step)+' / '+String(detail.limit));
      } else if (event.type==='instruction_applied') {
        row('지시 반영','다음 모델 단계에 적용됨');
      } else if (event.type==='finished') {
        row('실행 종료',statusText(detail.status));
      } else {
        row(event.type.replaceAll('_',' '),detail.model || detail.name || '기록 저장됨');
      }
      if (event.type==='finished' || event.type==='model_step' || event.type==='tool_result') {
        await updateTask();
        if (event.type==='finished') { stream?.close(); await loadTasks(); }
      }
    } catch (error) { notice(error.message); }
  };
  stream.onerror=()=>{ if (stream?.readyState===EventSource.CLOSED) notice('연결이 종료됐습니다. 작업 상태를 다시 읽을 수 있습니다.'); };
}
async function loadTasks() {
  const records=await api('/api/tasks');
  ui.tasks.replaceChildren();
  if (!records.length) {
    const box=document.createElement('div');
    box.className='empty';box.textContent='아직 실행한 작업이 없습니다.';
    ui.tasks.append(box);return;
  }
  for (const record of records) {
    const button=document.createElement('button');button.type='button';button.className='task';
    button.textContent=record.input?.prompt?.slice(0,130) || '제목 없는 작업';
    const info=document.createElement('span');
    info.textContent=statusText(record.status)+' · '+new Date(record.createdAt).toLocaleString('ko-KR');
    button.append(info);
    button.onclick=async()=>{
      active=record.id;renderTask(record);openStream(record.id);
      ui.work.scrollIntoView({behavior:'smooth',block:'start'});
    };
    ui.tasks.append(button);
  }
}
function submission() {
  return {
    requestId:crypto.randomUUID(),prompt:ui.prompt.value.trim(),
    sessionId:ui.sessionId.value.trim() || null,
    allowWrite:ui.write.checked,allowShell:ui.shell.checked,allowMcp:ui.mcp.checked,
    requireEvidence:ui.evidence.checked,
    maxSteps:Number(ui.maxSteps.value),tokenBudget:Number(ui.tokenBudget.value),
    planningMode:ui.planningMode.value
  };
}
async function create(retry=false) {
  if (!retry || !pending) pending=submission();
  if (!pending.prompt) return notice('작업 내용을 입력해 주세요.');
  ui.run.disabled=true;notice('');
  try {
    const task=await api('/api/tasks','POST',pending);
    pending=null;
    active=task.id;
    renderTask(task);
    openStream(task.id);
    await loadTasks();
    ui.work.scrollIntoView({behavior:'smooth',block:'nearest'});
  } catch (error) {
    notice(error.message+' · 같은 요청으로 다시 전송하려면 실행 시작을 누르세요.');
  } finally { ui.run.disabled=false; }
}
ui.form.addEventListener('submit',async event=>{event.preventDefault();await create(Boolean(pending));});
ui.refresh.addEventListener('click',async()=>{try{await loadTasks();await updateTask();notice('');}catch(e){notice(e.message);}});
ui.cancel.addEventListener('click',async()=>{
  if (!active) return;
  try{renderTask(await api('/api/tasks/'+encodeURIComponent(active)+'/cancel','POST',{}));await loadTasks();}
  catch(e){notice(e.message);}
});
ui.steer.addEventListener('click',async()=>{
  if (!active || !ui.instruction.value.trim()) return;
  try{
    await api('/api/tasks/'+encodeURIComponent(active)+'/steer','POST',{text:ui.instruction.value.trim()});
    ui.instruction.value='';notice('지시가 저장됐습니다. 다음 모델 단계에서 반영됩니다.');
  }catch(e){notice(e.message);}
});
async function loadMemories(){
  const session=byId('memoryScope').value==='session'
    ? byId('memorySession').value.trim():null;
  const query=session?'?sessionId='+encodeURIComponent(session):'';
  const items=await api('/api/memory'+query);
  const rows=byId('memoryRows');
  rows.replaceChildren();
  if(!items.length){
    const empty=document.createElement('p');empty.textContent='저장된 기억이 없습니다.';
    rows.append(empty);return;
  }
  for(const item of items){
    const card=document.createElement('div');card.className='task';
    const label=document.createElement('strong');
    label.textContent=(item.sessionId?'대화 전용':'공통')+' · '+item.kind;
    const content=document.createElement('p');
    content.textContent=item.content;
    const mute=document.createElement('button');
    mute.type='button';mute.className='ghost';
    mute.textContent=item.status==='active'?'이 기억 사용 중지':'비활성';
    mute.disabled=item.status!=='active';
    mute.addEventListener('click',async()=>{
      try{
        await api('/api/memory/'+encodeURIComponent(item.id)+'/disable','POST',{});
        await loadMemories();
      }catch(error){notice(error.message);}
    });
    card.append(label,content,mute);
    rows.append(card);
  }
}
async function loadSkills(){
  const items=await api('/api/skills');
  const rows=byId('skillRows');
  rows.replaceChildren();
  if(!items.length){
    const empty=document.createElement('p');empty.textContent='저장된 스킬이 없습니다.';
    rows.append(empty);return;
  }
  for(const item of items){
    const card=document.createElement('div');card.className='task';
    const title=document.createElement('strong');
    title.textContent=item.name+' · '+(item.trigger||'모든 대화');
    const description=document.createElement('p');
    description.textContent=item.instructions;
    const toggle=document.createElement('button');
    toggle.type='button';toggle.className='secondary';
    toggle.textContent=item.enabled?'스킬 비활성화':'검토 후 스킬 활성화';
    toggle.addEventListener('click',async()=>{
      try{
        await api('/api/skills/'+encodeURIComponent(item.id)+'/enable',
          'POST',{enabled:!item.enabled});
        await loadSkills();
      }catch(error){notice(error.message);}
    });
    card.append(title,description,toggle);
    rows.append(card);
  }
}
async function refreshKnowledge(){
  await Promise.all([loadMemories(),loadSkills()]);
}
byId('memoryCreate').addEventListener('submit',async event=>{
  event.preventDefault();
  const scope=byId('memoryScope').value;
  const session=scope==='session'?byId('memorySession').value.trim():null;
  if(scope==='session'&&!session){
    notice('대화 전용 기억을 저장하려면 대화 ID를 입력하세요.');return;
  }
  try{
    await api('/api/memory','POST',{
      content:byId('memoryContent').value.trim(),
      kind:byId('memoryKind').value,
      sessionId:session
    });
    byId('memoryContent').value='';
    await loadMemories();
    notice('기억이 저장됐습니다. 관련 대화에서 자동 검색됩니다.');
  }catch(error){notice(error.message);}
});
byId('skillCreate').addEventListener('submit',async event=>{
  event.preventDefault();
  try{
    await api('/api/skills','POST',{
      name:byId('skillName').value.trim(),
      trigger:byId('skillTrigger').value.trim(),
      instructions:byId('skillInstructions').value.trim(),
      enabled:false
    });
    byId('skillName').value='';
    byId('skillInstructions').value='';
    byId('skillTrigger').value='';
    await loadSkills();
    notice('스킬 초안이 저장됐습니다. 검토 후 활성화하세요.');
  }catch(error){notice(error.message);}
});
byId('memoryScope').addEventListener('change',async()=>{
  try{await loadMemories();}catch(error){notice(error.message);}
});
byId('memorySession').addEventListener('change',async()=>{
  if(byId('memoryScope').value==='session'){
    try{await loadMemories();}catch(error){notice(error.message);}
  }
});
byId('refreshKnowledge').addEventListener('click',async()=>{
  try{await refreshKnowledge();notice('');}catch(error){notice(error.message);}
});

let selectedImprovement = null;
let improvementBusy = false;

function viewImprovement(item){
  selectedImprovement=item?.id||null;
  const output=byId('improvementEvidence');
  if(!item){
    byId('improvementStatus').textContent='개선 실험을 선택하세요.';
    output.textContent='';
    byId('improvementEvaluate').disabled=true;
    byId('improvementAdopt').disabled=true;
    byId('improvementDescendant').disabled=true;
    return;
  }
  byId('improvementSelect').value=item.id;
  const status={
    awaiting_approval:'평가 대기',running:'실험 중',evaluated:'평가 완료',
    rejected:'개선 근거 부족',adopted:'채택 완료',failed:'실패'
  };
  byId('improvementStatus').textContent=
    (status[item.status]||item.status)+' · 예상 모델 비용 '+
    Number(item.cost_krw||0).toLocaleString('ko-KR')+'원';
  const summarize=measure=>measure
    ? (measure.tests_passed?'테스트 통과':'테스트 실패')+
       ' · 점수 '+(measure.score??'별도 점수 없음')+
       '\n'+String(measure.log||'').slice(0,800)
    : '측정 결과 없음';
  output.textContent=
    '저장소: '+item.repository+'\n수정 대상: '+item.target_file+
    '\n기준 커밋: '+(item.base_commit||'—')+
    '\n후보 커밋: '+(item.candidate_commit||'—')+
    '\n원본: '+summarize(item.baseline)+
    '\n후보: '+summarize(item.candidate)+
    '\n평가: '+(item.accepted?'채택 기준 충족':'아직 개선 판정 없음')+
    (item.error?'\n오류: '+item.error:'')+
    (item.reflection?'\n다음 세대 학습 메모: '+item.reflection.slice(0,1600):'');
  byId('improvementEvaluate').disabled=improvementBusy||
    item.status!=='awaiting_approval';
  byId('improvementAdopt').disabled=improvementBusy||
    item.status!=='evaluated'||!item.accepted;
  byId('improvementDescendant').disabled=improvementBusy||
    item.status!=='adopted';
}
async function refreshImprovements(preferred){
  const experiments=await api('/api/improvements');
  const picker=byId('improvementSelect');
  const current=preferred||selectedImprovement;
  picker.replaceChildren();
  const option=document.createElement('option');
  option.value='';option.textContent='실험을 선택하세요';picker.append(option);
  for(const item of experiments){
    const row=document.createElement('option');
    row.value=item.id;
    row.textContent=item.repository+'/'+item.target_file+' · '+item.status;
    picker.append(row);
  }
  if(current&&experiments.some(item=>item.id===current)){
    viewImprovement(experiments.find(item=>item.id===current));
  }else if(experiments.length)viewImprovement(experiments[0]);
  else viewImprovement(null);
}
byId('improvementCreate').addEventListener('submit',async event=>{
  event.preventDefault();if(improvementBusy)return;
  improvementBusy=true;
  try{
    const payload={
      repository:byId('improvementRepo').value.trim(),
      target_file:byId('improvementFile').value.trim(),
      instruction:byId('improvementInstruction').value.trim(),
      test_command:byId('improvementTest').value.trim(),
      score_command:byId('improvementScore').value.trim()||null
    };
    const created=await api('/api/improvements','POST',payload);
    await refreshImprovements(created.id);
    notice('개선 실험이 저장됐습니다. 승인 후 평가를 실행할 수 있습니다.');
  }catch(error){notice(error.message);}
  finally{improvementBusy=false;}
});
byId('improvementSelect').addEventListener('change',async event=>{
  if(!event.target.value){viewImprovement(null);return;}
  try{viewImprovement(await api('/api/improvements/'+encodeURIComponent(event.target.value)));}
  catch(error){notice(error.message);}
});
byId('improvementEvaluate').addEventListener('click',async()=>{
  if(!selectedImprovement||improvementBusy)return;
  if(!window.confirm('격리된 Git 작업공간에서 모델과 검증 명령을 실행합니다. 진행하시겠습니까?'))return;
  improvementBusy=true;byId('improvementEvaluate').disabled=true;
  try{
    viewImprovement(await api('/api/improvements/'+encodeURIComponent(selectedImprovement)+
      '/evaluate','POST',{}));
    notice('원본·후보 비교를 시작했습니다. 평가 결과 확인을 사용하세요.');
  }catch(error){notice(error.message);}
  finally{improvementBusy=false;}
});
byId('improvementRefresh').addEventListener('click',async()=>{
  if(!selectedImprovement)return;
  try{
    viewImprovement(await api('/api/improvements/'+encodeURIComponent(selectedImprovement)));
    notice('');
  }catch(error){notice(error.message);}
});
byId('improvementAdopt').addEventListener('click',async()=>{
  if(!selectedImprovement||improvementBusy)return;
  if(!window.confirm('채택한 변경은 원본 Git 저장소에 반영됩니다. 실제로 적용하시겠습니까?'))return;
  improvementBusy=true;
  try{
    viewImprovement(await api('/api/improvements/'+encodeURIComponent(selectedImprovement)+
      '/adopt','POST',{}));
    notice('평가된 변경을 원본에 적용했습니다.');
  }catch(error){notice(error.message);}
  finally{improvementBusy=false;}
});
byId('improvementDescendant').addEventListener('click',async()=>{
  if(!selectedImprovement||improvementBusy)return;
  improvementBusy=true;
  try{
    const next=await api('/api/improvements/'+encodeURIComponent(selectedImprovement)+
      '/descendant','POST',{});
    await refreshImprovements(next.id);
    notice('앞선 평가 경험을 이어받은 다음 세대 실험을 만들었습니다.');
  }catch(error){notice(error.message);}
  finally{improvementBusy=false;}
});
byId('refreshImprovements').addEventListener('click',async()=>{
  try{await refreshImprovements();notice('');}catch(error){notice(error.message);}
});

let selectedVideo = null;
let videoBusy = false;

function viewVideo(video) {
  selectedVideo=video?.id||null;
  const items=byId('videoScenes');
  items.replaceChildren();
  const link=byId('videoDownload');
  link.hidden=true;
  if(!video){
    byId('videoStatus').textContent='새 영상 프로젝트를 만들거나 기존 영상을 선택하세요.';
    byId('videoRender').disabled=true;
    return;
  }
  byId('videoSelect').value=video.id;
  const status={draft:'장면 준비',rendering:'렌더링 중',completed:'완성',failed:'실패'};
  byId('videoStatus').textContent=(status[video.status]||video.status)+
    ' · '+(video.scenes||[]).length+'개 장면 · 예상 API 비용 '+
    Number(video.cost_krw||0).toLocaleString('ko-KR')+'원';
  if(video.error)notice(video.error);
  byId('videoRender').disabled=videoBusy||video.status==='rendering';
  if(video.status==='completed'){
    link.hidden=false;
    link.href='/api/videos/'+encodeURIComponent(video.id)+'/content';
    link.download='dominion-'+video.id+'.mp4';
  }
  (video.scenes||[]).forEach((scene,index)=>{
    const section=document.createElement('div');
    section.className='advanced';
    const head=document.createElement('strong');
    head.textContent=(index+1)+'번 장면';
    const title=document.createElement('input');
    title.value=scene.heading;title.maxLength=180;
    title.setAttribute('aria-label',(index+1)+'번 장면 제목');
    const narration=document.createElement('input');
    narration.value=scene.narration;narration.maxLength=1000;
    narration.setAttribute('aria-label',(index+1)+'번 장면 내용');
    const seconds=document.createElement('input');
    seconds.type='number';seconds.min=2;seconds.max=30;seconds.value=scene.seconds;
    seconds.setAttribute('aria-label',(index+1)+'번 장면 초');
    const save=document.createElement('button');
    save.type='button';save.className='ghost';save.textContent='장면 수정 저장';
    save.disabled=video.status==='rendering';
    save.addEventListener('click',async()=>{
      if(videoBusy)return;
      videoBusy=true;save.disabled=true;
      try{
        const changed=await api('/api/videos/'+encodeURIComponent(video.id)+
          '/scenes/'+index,'POST',{
            heading:title.value.trim(),narration:narration.value,
            seconds:Number(seconds.value)
          });
        viewVideo(changed);
        notice('바뀐 장면만 다시 렌더링하도록 저장했습니다.');
      }catch(error){notice(error.message);}
      finally{videoBusy=false;save.disabled=false;}
    });
    section.append(head,title,narration,seconds,save);
    items.append(section);
  });
}
async function refreshVideos(preferred){
  const videos=await api('/api/videos');
  const picker=byId('videoSelect');
  const chosen=preferred||selectedVideo;
  picker.replaceChildren();
  const first=document.createElement('option');
  first.value='';first.textContent='영상을 선택하세요';picker.append(first);
  for(const video of videos){
    const item=document.createElement('option');
    item.value=video.id;
    item.textContent=video.prompt.slice(0,70)+' · '+video.status;
    picker.append(item);
  }
  if(chosen && videos.some(video=>video.id===chosen)){
    viewVideo(videos.find(video=>video.id===chosen));
  }else if(videos.length)viewVideo(videos[0]);
  else viewVideo(null);
}
byId('videoCreate').addEventListener('submit',async event=>{
  event.preventDefault();
  if(videoBusy)return;
  const backend=byId('videoBackend').value;
  if(backend==='veo' && !window.confirm(
    'Veo는 외부 영상 생성 API를 호출하며 요금이 청구될 수 있습니다. 계속하시겠습니까?'))return;
  videoBusy=true;
  try{
    const payload={
      prompt:byId('videoPrompt').value.trim(),backend
    };
    const heading=byId('videoHeading').value.trim();
    if(heading){
      payload.scenes=[{heading,narration:byId('videoNarration').value,
        seconds:Number(byId('videoSeconds').value)}];
    }
    const video=await api('/api/videos','POST',payload);
    await refreshVideos(video.id);
    notice('영상 프로젝트가 저장됐습니다. 내용을 검토하고 렌더링을 시작하세요.');
  }catch(error){notice(error.message);}
  finally{videoBusy=false;}
});
byId('videoSelect').addEventListener('change',async event=>{
  if(!event.target.value){viewVideo(null);return;}
  try{viewVideo(await api('/api/videos/'+encodeURIComponent(event.target.value)));}
  catch(error){notice(error.message);}
});
byId('videoRender').addEventListener('click',async()=>{
  if(!selectedVideo||videoBusy)return;
  videoBusy=true;byId('videoRender').disabled=true;
  try{
    const latest=await api('/api/videos/'+encodeURIComponent(selectedVideo));
    if(latest.backend==='veo' && !window.confirm(
      'Veo 렌더링은 영상 생성 비용이 발생할 수 있습니다. 시작하시겠습니까?'))return;
    viewVideo(await api('/api/videos/'+encodeURIComponent(selectedVideo)+'/render',
      'POST',{}));
    notice('렌더링 작업을 시작했습니다. 상태 확인을 눌러 결과를 확인하세요.');
  }catch(error){notice(error.message);}
  finally{videoBusy=false;}
});
byId('videoCheck').addEventListener('click',async()=>{
  if(!selectedVideo)return;
  try{viewVideo(await api('/api/videos/'+encodeURIComponent(selectedVideo)));notice('');}
  catch(error){notice(error.message);}
});
byId('refreshVideos').addEventListener('click',async()=>{
  try{await refreshVideos();notice('');}catch(error){notice(error.message);}
});

let selectedGroup = null;
let groupBusy = false;

function viewGroup(group) {
  selectedGroup = group?.id || null;
  const out=byId('groupMessages');
  out.replaceChildren();
  if (!group) {
    byId('groupStatus').textContent='새 그룹을 만들거나 기존 그룹을 선택하세요.';
    return;
  }
  byId('groupSelect').value=group.id;
  byId('groupStatus').textContent=
    (group.status==='open'?'대화 중':'종료')+' · '+
    group.rounds+'/'+group.max_rounds+'회 · 예상 비용 '+
    Number(group.cost_krw||0).toLocaleString('ko-KR')+'원';
  for (const message of group.messages || []) {
    const item=document.createElement('li');
    const heading=document.createElement('strong');
    heading.textContent=(message.speaker==='user'?'사용자':message.speaker)+' · ';
    const content=document.createElement('span');
    content.textContent=message.content;
    item.append(heading,content);
    out.append(item);
  }
  byId('groupNext').disabled=groupBusy||group.status!=='open';
}
async function refreshGroups(preferred) {
  const rooms=await api('/api/groups');
  const picker=byId('groupSelect');
  const value=preferred || selectedGroup;
  picker.replaceChildren();
  const placeholder=document.createElement('option');
  placeholder.value='';placeholder.textContent='그룹을 선택하세요';
  picker.append(placeholder);
  for(const room of rooms) {
    const option=document.createElement('option');
    option.value=room.id;
    option.textContent=room.topic+' · '+room.participants.map(p=>p.name).join(', ');
    picker.append(option);
  }
  if(value && rooms.some(room=>room.id===value)){
    viewGroup(rooms.find(room=>room.id===value));
  }else if(rooms.length) viewGroup(rooms[0]);
  else viewGroup(null);
}
byId('groupCreate').addEventListener('submit',async event=>{
  event.preventDefault();
  if(groupBusy)return;
  const a=byId('groupNameA').value.trim(),b=byId('groupNameB').value.trim();
  if(!a||!b||a.toLowerCase()===b.toLowerCase()){
    notice('그룹 참가자 이름은 서로 달라야 합니다.');return;
  }
  groupBusy=true;
  try{
    const room=await api('/api/groups','POST',{
      topic:byId('groupTopic').value.trim(),
      participants:[
        {name:a,role:byId('groupRoleA').value.trim(),economy:false},
        {name:b,role:byId('groupRoleB').value.trim(),economy:true}
      ],
      max_rounds:12,budget_krw:Number(byId('groupBudget').value)
    });
    await refreshGroups(room.id);
    notice('그룹이 저장됐습니다. 먼저 메시지를 보내고 다음 발언을 실행하세요.');
  }catch(error){notice(error.message);}
  finally{groupBusy=false;}
});
byId('groupSelect').addEventListener('change',async event=>{
  const id=event.target.value;
  if(!id){viewGroup(null);return;}
  try{viewGroup(await api('/api/groups/'+encodeURIComponent(id)));}
  catch(error){notice(error.message);}
});
byId('groupSpeak').addEventListener('submit',async event=>{
  event.preventDefault();
  if(groupBusy||!selectedGroup){notice('먼저 그룹을 선택하세요.');return;}
  groupBusy=true;
  try{
    const text=byId('groupInput').value.trim();
    if(!text)return;
    const room=await api('/api/groups/'+encodeURIComponent(selectedGroup)+'/messages',
      'POST',{content:text});
    byId('groupInput').value='';
    viewGroup(room);
    notice('메시지가 전달됐습니다.');
  }catch(error){notice(error.message);}
  finally{groupBusy=false;}
});
byId('groupNext').addEventListener('click',async()=>{
  if(!selectedGroup||groupBusy)return;
  groupBusy=true;byId('groupNext').disabled=true;
  notice('에이전트가 저장된 대화를 읽고 발언하고 있습니다.');
  try{
    viewGroup(await api('/api/groups/'+encodeURIComponent(selectedGroup)+'/next','POST',{}));
    notice('');
  }catch(error){notice(error.message);}
  finally{groupBusy=false;if(selectedGroup)byId('groupNext').disabled=false;}
});
byId('refreshGroups').addEventListener('click',async()=>{
  try{await refreshGroups();notice('');}catch(error){notice(error.message);}
});

(async()=>{
  try{
    const status=await api('/api/health');
    ui.engineLed.classList.add('on');
    ui.connection.textContent='Rust 연결됨 · '+status.model;
    await loadTasks();
    await refreshGroups();
    await refreshVideos();
    await refreshImprovements();
    await refreshKnowledge();
  }catch(error){
    ui.connection.textContent='서버 연결 끊김';notice(error.message);
  }
})();
