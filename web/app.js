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
(async()=>{
  try{
    const status=await api('/api/health');
    ui.engineLed.classList.add('on');
    ui.connection.textContent='Rust 연결됨 · '+status.model;
    await loadTasks();
  }catch(error){
    ui.connection.textContent='서버 연결 끊김';notice(error.message);
  }
})();
