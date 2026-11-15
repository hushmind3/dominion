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
  }catch(error){
    ui.connection.textContent='서버 연결 끊김';notice(error.message);
  }
})();
