use crate::engine::jobs::JobContext;
use crate::engine::{chat::TurnRequest, run_room_quiet};
use crate::state::AppState;
use serde_json::{json, Value};
use std::collections::{HashSet, VecDeque};

#[path = "room_parallel.rs"]
mod parallel;

pub const MAX_PARTICIPANTS: usize = 32;
/// Safety ceiling only. The room's own rounds/maxTurns decide the budget;
/// operators can raise this without a code change.
fn max_turns_ceiling() -> usize {
    std::env::var("DOMINION_MAX_ROOM_TURNS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(100_000)
}

/// One durable transcript belongs to each room. It is shared by the owner and
/// every participant; changing the selected speaker never changes this binding.
pub fn ensure_room_session(state: &AppState, room_id: &str) -> Result<Value, String> {
    let room = state
        .store
        .get("room", room_id)
        .ok_or("대화방을 찾을 수 없습니다.")?;
    let session_id = room
        .get("sessionId")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("room:{room_id}"));
    let session = match state.store.get("session", &session_id) {
        Some(mut session) => {
            if session
                .get("roomId")
                .and_then(Value::as_str)
                .is_some_and(|id| id != room_id)
                || session
                    .get("channelContext")
                    .is_some_and(|value| !value.is_null())
            {
                return Err("다른 대화방 또는 외부 채널의 세션을 연결할 수 없습니다.".into());
            }
            if session.get("roomId").is_none_or(Value::is_null) {
                // A pre-group implementation could save an untyped session.
                // Do not adopt an ordinary private chat merely because its id
                // was entered into a room record.
                let expected = format!("room:{room_id}");
                if session_id != expected {
                    return Err("대화방 소유권이 없는 기존 세션은 연결할 수 없습니다.".into());
                }
                session["roomId"] = json!(room_id);
                state
                    .store
                    .put("session", &session)
                    .ok_or("대화방 세션을 저장할 수 없습니다.")?
            } else {
                session
            }
        }
        None => {
            let record = json!({"id":session_id,"title":room.get("name").cloned().unwrap_or(json!("대화방")),"roomId":room_id,"kind":"room"});
            match state.store.put_if_absent("session", &record) {
                Some(session) => session,
                None => state
                    .store
                    .get("session", &session_id)
                    .filter(|session| session["roomId"].as_str() == Some(room_id))
                    .ok_or("대화방 세션을 만들 수 없습니다.")?,
            }
        }
    };
    if room["sessionId"].as_str() != Some(session_id.as_str()) {
        // Stable GETs must remain read-only: emitting a room record here makes
        // event subscribers refetch /state, which otherwise emits it again.
        // Compare the binding itself so concurrent first reads also avoid a
        // redundant write/event once another reader has established it.
        match state.store.patch_if(
            "room",
            room_id,
            "sessionId",
            room.get("sessionId").unwrap_or(&Value::Null),
            &json!({"sessionId":session_id}),
        ) {
            Some(saved) => state.bus.record("room", &saved),
            None if state
                .store
                .get("room", room_id)
                .is_some_and(|room| room["sessionId"].as_str() == Some(session_id.as_str())) => {}
            None => return Err("대화방 세션 연결을 저장할 수 없습니다.".into()),
        }
    }
    Ok(session)
}

/// Choose initial responders. Discussion visits the roster, round robin picks
/// the next member, and mentioned waits for a real @name/@id. Explicit owner
/// selections override the initial routing, without changing room membership.
fn is_deep_request(input: &str) -> bool {
    let lower = input.to_lowercase();
    input.chars().count() > 240
        || [
            "분석", "설계", "구현", "검토", "조사", "테스트", "버그", "최적화", "리팩터",
            "원인", "해결", "비교", "계획", "문서", "아키텍처", "리뷰", "debug", "refactor",
            "design", "analyze", "investigate",
        ]
        .iter()
        .any(|term| lower.contains(term))
}

/// Chooses the smallest useful cohort for a discussion turn. The owner can
/// still force everyone with "다들/모두/전원"; mentions override this entirely.
fn speaker_window(
    state: &AppState,
    room: &Value,
    input: &str,
    roster: &[String],
) -> Vec<String> {
    // Small rooms are cheap enough to keep everyone; trimming them loses more
    // perspective than it saves.
    if roster.len() <= 3 {
        return roster.to_vec();
    }
    // An explicit total cap of at least one turn per participant is the
    // owner's instruction that everyone should speak. Do not silently trim
    // that room down to two casual responders.
    if room["maxTurns"]
        .as_u64()
        .is_some_and(|turns| turns as usize >= roster.len())
    {
        return roster.to_vec();
    }
    let lower = input.to_lowercase();
    if [
        "다들", "다 같이", "다같이", "모두", "모든", "전원", "전부", "각자",
        "나머지", "나머지도", "아홉", "9명", "인사", "everyone", "all agents",
    ]
        .iter()
        .any(|term| lower.contains(term))
    {
        return roster.to_vec();
    }
    let count = if is_deep_request(input) {
        6.min(roster.len())
    } else if input.chars().count() <= 60 {
        2.min(roster.len())
    } else {
        3.min(roster.len())
    };
    if count >= roster.len() {
        return roster.to_vec();
    }
    let mut terms: Vec<String> = input
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|word| word.chars().count() >= 2)
        .map(|word| word.to_lowercase())
        .collect();
    if let Some(topic) = room["topic"].as_str() {
        terms.extend(
            topic
                .split(|ch: char| !ch.is_alphanumeric())
                .filter(|word| word.chars().count() >= 2)
                .map(|word| word.to_lowercase()),
        );
    }
    terms.sort();
    terms.dedup();
    let mut scored: Vec<(usize, usize, String)> = roster
        .iter()
        .enumerate()
        .map(|(index, id)| {
            let agent = state.store.get("agent", id).unwrap_or(Value::Null);
            let mut text = String::new();
            for key in ["name", "description", "role", "systemPrompt"] {
                if let Some(value) = agent[key].as_str() {
                    text.push_str(value);
                    text.push(' ');
                }
            }
            for alias in agent["aliases"].as_array().into_iter().flatten() {
                if let Some(alias) = alias.as_str() {
                    text.push_str(alias);
                    text.push(' ');
                }
            }
            let text = text.to_lowercase();
            let score = terms
                .iter()
                .filter(|term| text.contains(term.as_str()))
                .count();
            (score + usize::from(index == 0), index, id.clone())
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    let mut selected: Vec<usize> = scored.into_iter().take(count).map(|row| row.1).collect();
    if !selected.contains(&0) {
        if let Some(last) = selected.last_mut() {
            *last = 0;
        }
    }
    selected.sort_unstable();
    selected.into_iter().map(|index| roster[index].clone()).collect()
}

/// Stable model assignment by participant order. Different participants get
/// different pool entries whenever the pool is large enough, and the same
/// participant keeps its entry for every turn of the room.
fn pool_slot(roster: &[String], agent_id: &str, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    roster
        .iter()
        .position(|id| id == agent_id)
        .unwrap_or(0)
        % len
}

fn auto_reasoning_effort(input: &str) -> String {
    if is_deep_request(input) {
        "medium".into()
    } else if input.chars().count() <= 60 {
        "none".into()
    } else {
        "low".into()
    }
}

pub fn plan_speakers(
    state: &AppState,
    room: &Value,
    input: &str,
    explicit: Option<&Value>,
) -> Result<Vec<String>, String> {
    let roster = participants(room)?;
    for id in &roster {
        let agent = state
            .store
            .get("agent", id)
            .ok_or_else(|| format!("참가 에이전트를 찾을 수 없습니다: {id}"))?;
        if agent["enabled"].as_bool() == Some(false) {
            return Err(format!(
                "비활성 에이전트가 참가자로 지정되어 있습니다: {id}"
            ));
        }
    }
    let mode = routing_mode(state, room)?;
    if let Some(value) = explicit {
        let selected = id_list(value)?;
        if selected.iter().any(|id| !roster.contains(id)) {
            return Err("응답자는 대화방 참가자 중에서 선택하세요.".into());
        }
        return Ok(selected);
    }
    let mentioned = mentioned_agents(state, &roster, input)?;
    match mode.as_str() {
        "discussion" => {
            if !mentioned.is_empty() {
                return Ok(mentioned);
            }
            Ok(speaker_window(state, room, input, &roster))
        }
        "round_robin" => {
            if !mentioned.is_empty() {
                return Ok(mentioned);
            }
            let next = room["nextSpeakerIndex"].as_u64().unwrap_or(0) as usize % roster.len();
            Ok(vec![roster[next].clone()])
        }
        "mentioned" => Ok(mentioned),
        _ => unreachable!(),
    }
}

/// Each job responds to one actual owner message in the shared transcript.
/// Model instructions never create extra user messages or turn peer claims
/// into owner facts. Autonomous exchanges end at a finite turn budget.
/// Distinct provider/model pairs from the room's participants. This is
/// generic: any room size reuses the same pool and rotates per turn instead
/// of hardcoding named agents.
fn model_pool(state: &crate::state::AppState, room: &Value) -> Vec<(String, String, Option<String>)> {
    let mut pool = Vec::new();
    let mut seen = std::collections::HashSet::new();
    // An explicit owner selection wins over agent defaults. This is how the
    // owner says "use these Fast FreeLLMAPI models, never anything else".
    for entry in room["models"].as_array().into_iter().flatten() {
        let provider = entry["providerId"].as_str().unwrap_or("");
        let model = entry["model"].as_str().unwrap_or("");
        let effort = entry["reasoningEffort"].as_str().filter(|value| !value.is_empty()).map(str::to_string);
        if provider.is_empty() || model.is_empty() {
            continue;
        }
        let key = format!("{provider}|{model}");
        if seen.insert(key) {
            pool.push((provider.to_string(), model.to_string(), effort));
        }
    }
    if !pool.is_empty() {
        return pool;
    }
    for id in room["agentIds"].as_array().into_iter().flatten() {
        let Some(agent) = id.as_str().and_then(|id| state.store.get("agent", id)) else {
            continue;
        };
        let provider = agent["providerId"].as_str().unwrap_or("");
        let model = agent["model"].as_str().unwrap_or("");
        if provider.is_empty() || model.is_empty() {
            continue;
        }
        let key = format!("{provider}|{model}");
        if seen.insert(key) {
            pool.push((provider.to_string(), model.to_string(), None));
        }
    }
    pool
}

pub async fn run(context: &JobContext, job: &Value) -> Result<Value, String> {
    let input = job.get("input").cloned().unwrap_or_else(|| json!({}));
    let room_id = input["roomId"].as_str().ok_or("대화방 ID가 필요합니다.")?;
    let lock_id = format!("room-run:{room_id}");
    if !context.state.begin_session(&lock_id) {
        return Err("deferred: room conversation is already running".into());
    }
    let _guard = context.state.session_guard(&lock_id);
    let result = run_locked(context, job, &input, room_id).await;
    if let Err(error) = &result {
        if !error.starts_with("deferred:") {
            let current = context
                .state
                .store
                .get("room", room_id)
                .unwrap_or(Value::Null);
            if current["lastJobId"].as_str() == Some(context.job_id.as_str()) {
                let cancelled = job_cancelled(context);
                patch_room(
                    &context.state,
                    room_id,
                    json!({"status":if cancelled {"paused"} else {"needs_review"},"activeAgentId":null,"pauseReason":if cancelled {"stopped"} else {"execution_failed"},"lastError":error}),
                );
            }
        }
    }
    result
}

async fn run_locked(
    context: &JobContext,
    job: &Value,
    input: &Value,
    room_id: &str,
) -> Result<Value, String> {
    let state = &context.state;
    let current_room = state
        .store
        .get("room", room_id)
        .ok_or("대화방을 찾을 수 없습니다.")?;
    // Permissions and membership are the owner's configuration when this
    // message was submitted, never a later model-supplied policy.
    let room = input
        .get("roomSnapshot")
        .or_else(||job.pointer("/roomBinding/roomSnapshot"))
        .filter(|value| value.is_object())
        .cloned()
        .unwrap_or_else(|| current_room.clone());
    if room
        .get("id")
        .and_then(Value::as_str)
        .is_some_and(|id| id != room_id)
    {
        return Err("대화방 설정 스냅샷이 다른 방을 가리킵니다.".into());
    }
    let existing = job.get("roomState").cloned().unwrap_or_else(|| json!({}));
    if existing["inFlight"].as_bool() == Some(true) {
        patch_room(
            state,
            room_id,
            json!({"status":"needs_review","activeAgentId":null,"pauseReason":"uncertain_turn"}),
        );
        return Ok(
            json!({"roomId":room_id,"sessionId":existing.get("sessionId"),"status":"needs_review","reason":"중단된 발언 또는 도구 작업은 확인 없이 반복하지 않습니다."}),
        );
    }
    let session = ensure_room_session(state, room_id)?;
    let session_id = session["id"].as_str().ok_or("대화방 세션 ID가 없습니다.")?;
    if input
        .get("sessionId")
        .and_then(Value::as_str)
        .is_some_and(|id| id != session_id)
    {
        return Err("작업이 다른 대화방 세션을 가리킵니다.".into());
    }
    let generation = input
        .get("generation")
        .or_else(|| existing.get("generation"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    // A real owner stop advances the durable epoch and cancels the job.
    // A SQLite resume failure at the *same* epoch is NOT a user cancellation.
    // Record the distinction so a two-second empty group run is diagnosable.
    if job_cancelled(context) || state.cancellation_generation(session_id)!=generation {
        let reason=if job_cancelled(context){"job_cancel_requested"}
            else{"session_epoch_changed"};
        return Ok(finish(context,room_id,session_id,"cancelled",&[],reason,None));
    }
    if !state.resume_session_if_generation(session_id, generation) {
        if state.cancellation_generation(session_id)!=generation {
            return Ok(finish(context,room_id,session_id,"cancelled",&[],
                "session_epoch_changed",None));
        }
        return Err("그룹 세션 취소 세대를 복구하는 저장소 작업이 실패했습니다.".into());
    }

    // The queued owner's `input` is immutable under the native job checkpoint
    // CAS. Earlier group runs tried to rewrite `input` and were rejected
    // BEFORE the first model call. Freeze compatibility room bindings in a
    // separate field, leaving owner text and permissions untouched.
    let mut room_binding=json!({
        "sessionId":session_id,"generation":generation
    });
    // The owner-message API already persists its immutable snapshot in input.
    // Legacy /run jobs have none, so freeze it once here for safe resume.
    if input.get("roomSnapshot").is_none(){
        room_binding["roomSnapshot"]=room.clone();
    }
    context.checkpoint(json!({"roomBinding":room_binding}))?;

    let source = owner_message(context, input, session_id, room_id)?;
    let source_id = source["id"].as_str().ok_or("주인 메시지 ID가 없습니다.")?;
    let owner_text = source["content"].as_str().unwrap_or("");
    // One root task per owner utterance. Every speaker below is an attempt on
    // this root, and peers' replies are observations, not new owner requests.
    let task_id = format!("room-task-{source_id}");
    let root = crate::engine::episode::start_root(
        state,
        crate::engine::episode::RootSpec {
            task_id: &task_id,
            intent: owner_text,
            family: "room",
            source: "owner",
            source_message_id: Some(source_id),
            scope: json!({"partition": "local", "sessionId": session_id, "roomId": room_id}),
            mandatory: source["metadata"]["acceptance"]["checks"]
                .as_array()
                .cloned()
                .unwrap_or_default(),
        },
    );
    let task_revision = root
        .as_ref()
        .and_then(crate::engine::episode::contract_revision)
        .map(str::to_string);
    room_binding["taskId"]=json!(task_id);
    room_binding["taskRevision"]=json!(task_revision);
    room_binding["sourceMessageId"]=json!(source_id);
    context.checkpoint(json!({"roomBinding":room_binding}))?;
    let roster = if let Some(value) = existing.get("roster") {
        id_list(value)?
    } else {
        participants(&room)?
    };
    if roster.is_empty() {
        return Err("대화방 참가자가 없습니다.".into());
    }
    let mode = existing["routingMode"]
        .as_str()
        .or_else(|| input["routingMode"].as_str())
        .map(str::to_string)
        .unwrap_or(routing_mode(state, &room)?);
    if !matches!(mode.as_str(), "discussion" | "round_robin" | "mentioned") {
        return Err("지원하지 않는 대화방 응답 방식입니다.".into());
    }
    let initial = if let Some(value) = existing.get("initialSpeakers") {
        id_list(value)?
    } else {
        let explicit = input["explicitAgentIds"]
            .as_bool()
            .unwrap_or(input.get("agentIds").is_some());
        let mut routing = room.clone();
        if mode == "round_robin" {
            routing["nextSpeakerIndex"] = current_room["nextSpeakerIndex"].clone();
        }
        let selection = if mode == "round_robin" && !explicit {
            None
        } else {
            input.get("agentIds")
        };
        plan_speakers(state, &routing, owner_text, selection)?
    };
    if initial.iter().any(|id| !roster.contains(id)) {
        return Err("실행 체크포인트의 응답자가 대화방 참가자가 아닙니다.".into());
    }
    let rounds = room["rounds"].as_i64().unwrap_or(3).clamp(1, max_turns_ceiling() as i64) as usize;
    // rounds is the primary control: one round means every participant
    // speaks once. maxTurns > 0 is an explicit total cap; 0 means automatic.
    let budget = existing["turnBudget"]
        .as_u64()
        .map(|value| value as usize)
        .unwrap_or_else(|| {
            room["maxTurns"]
                .as_i64()
                .filter(|value| *value > 0)
                .map(|value| value as usize)
                .unwrap_or(roster.len() * rounds)
        })
        .clamp(1, max_turns_ceiling());
    let token_budget = room["tokenBudget"].as_u64().unwrap_or(0);
    // Resume must charge the entire owner request, not reset the quota at
    // every process restart or checkpoint replay.
    let usage_baseline=existing["tokenUsageBaseline"].as_u64()
        .unwrap_or_else(||state.store.session_usage(session_id)["totalTokens"]
            .as_u64().unwrap_or(0));
    let conflict_strategy =
        crate::engine::algorithms::variant_for_agent(state, "multiagent.conflict", None);
    let mut conflict_resolution = existing
        .get("conflictResolution")
        .cloned()
        .unwrap_or(Value::Null);
    let mut outputs = existing["outputs"].as_array().cloned().unwrap_or_default();
    if outputs.len() > budget {
        return Err("대화방 발언 체크포인트가 허용 횟수를 초과했습니다.".into());
    }
    let mut round_slots = 0usize;
    let mut cohort: Vec<String> = Vec::new();
    let mut pending: VecDeque<String> = if let Some(value) = existing.get("pendingSpeakers") {
        id_list_with_repeats(value)?.into()
    } else {
        let count = if mode == "discussion" { rounds } else { 1 };
        (0..count)
            .flat_map(|_| initial.iter().cloned())
            .skip(outputs.len())
            .take(budget.saturating_sub(outputs.len()))
            .collect()
    };
    if pending.iter().any(|id| !roster.contains(id)) {
        return Err("대화방 발언 순서에 없는 참가자가 포함되어 있습니다.".into());
    }
    // Parallel mode is explicit, owner-controlled, and only accepted for one
    // independent, read-only wave. Tool-enabled work and discussion follow the
    // original serial approval, context and durable checkpoint path.
    if room["executionMode"]=="parallel_readonly" && room["toolsEnabled"]!=true
        && rounds==1 && initial.len()>=2 && initial.len()<=8
        && initial.len()==budget && pending.len()==initial.len()
        && outputs.is_empty()
        && (token_budget==0 || token_budget>=initial.len() as u64 * 1024)
    {
        patch_room(state,room_id,json!({
            "status":"running","activeAgentId":null,
            "lastJobId":context.job_id,
            "lastSourceMessageId":source_id,
            "pauseReason":null,"lastError":null,
        }));
        checkpoint(context,session_id,generation,source_id,&mode,&roster,
            &initial,&pending,&outputs,budget,true,None)?;
        let explicit_pool=room["models"].as_array().is_some_and(|models|!models.is_empty());
        let route_pool=if explicit_pool||room["autoModels"].as_bool().unwrap_or(true){
            model_pool(state,&room)
        }else{Vec::new()};
        return parallel::run(
            context,&room,input,room_id,session_id,source_id,owner_text,
            &task_id,generation,&mode,&roster,&initial,budget,
            &route_pool,token_budget,
        ).await;
    }
    patch_room(
        state,
        room_id,
        json!({"status":"running","activeAgentId":null,"lastJobId":context.job_id,"lastSourceMessageId":source_id,"pauseReason":null,"lastError":null}),
    );
    checkpoint(
        context, session_id, generation, source_id, &mode, &roster, &initial, &pending, &outputs,
        budget, false, None,
    )?;

    while outputs.len() < budget {
        if mode == "discussion" && (cohort.is_empty() || round_slots >= roster.len()) {
            round_slots = 0;
            // Mentions since the owner's message define who may speak this
            // round. Recomputed each round so a member pulled in later joins.
            let mut text = owner_text.to_string();
            for output in &outputs {
                if let Some(content) = output["content"].as_str() {
                    text.push('\n');
                    text.push_str(content);
                }
            }
            let mentioned = mentioned_agents(state, &roster, &text).unwrap_or_default();
            cohort = if mentioned.is_empty() { roster.clone() } else { mentioned };
        }
        if stopped(context, session_id, generation) {
            return Ok(finish(
                context,
                room_id,
                session_id,
                "cancelled",
                &outputs,
                stop_reason(context,session_id,generation).unwrap_or("session_cancelled"),
                None,
            ));
        }
        if let Some(newer) = newer_owner_message(state, session_id, source_id) {
            return Ok(finish(
                context,
                room_id,
                session_id,
                "completed",
                &outputs,
                "new_owner_message",
                Some(&newer),
            ));
        }
        let Some(agent_id) = pending.pop_front() else {
            break;
        };
        round_slots += 1;
        if mode == "discussion" && !cohort.contains(&agent_id) {
            context.emit(json!({"type":"silence","roomId":room_id,"agentId":agent_id,"reason":"not_mentioned"}));
            continue;
        }
        let agent = state
            .store
            .get("agent", &agent_id)
            .ok_or_else(|| format!("참가 에이전트가 삭제되었습니다: {agent_id}"))?;
        if agent["enabled"].as_bool() == Some(false) {
            return Err(format!("참가 에이전트가 비활성 상태입니다: {agent_id}"));
        }
        let name = agent["name"].as_str().unwrap_or(&agent_id);
        context.emit(json!({"type":"speaker","roomId":room_id,"agentId":agent_id,"name":name,"step":outputs.len()+1,"stepTotal":budget}));
        patch_room(
            state,
            room_id,
            json!({"status":"running","activeAgentId":agent_id}),
        );
        let work_assignment=if room["toolsEnabled"]==true && roster.len()>=2 {
            Some(crate::engine::room_workboard::assign(state,room_id,
                session_id,source_id,&task_id,&agent_id,outputs.len()+1)?)
        }else{None};
        // Include the selected speaker in the saved pending queue until the
        // result is durable; any interrupted inFlight turn requires review.
        let mut saved_pending = pending.clone();
        saved_pending.push_front(agent_id.clone());
        checkpoint(
            context,
            session_id,
            generation,
            source_id,
            &mode,
            &roster,
            &initial,
            &saved_pending,
            &outputs,
            budget,
            true,
            Some(&agent_id),
        )?;
        // An explicit owner selection is itself a rotation list. Choosing
        // models in the room UI is the owner saying "use exactly these".
        let explicit_pool = room["models"]
            .as_array()
            .is_some_and(|models| !models.is_empty());
        let pool = if explicit_pool || room["autoModels"].as_bool().unwrap_or(true) {
            model_pool(state, &room)
        } else {
            Vec::new()
        };
        // Keep one model per agent for the whole room. Stable routes preserve
        // the prompt prefix (provider caching) and make costs predictable;
        // rate limits are still spread because different agents hash to
        // different pool entries.
        let rotated = if pool.is_empty() {
            None
        } else {
            Some(pool[pool_slot(&roster, &agent_id, pool.len())].clone())
        };
        let resolved_provider=optional(input,"providerId").or_else(||rotated.as_ref().map(|(provider,_,_)| provider.clone())).or_else(||optional(&agent,"providerId"));
        let resolved_model=optional(input,"model").or_else(||rotated.as_ref().map(|(_,model,_)| model.clone())).or_else(||optional(&agent,"model"));
        let rotated_effort=rotated.as_ref().and_then(|(_,_,effort)| effort.clone());
        // The owner's room model selection is a routing decision, not a hint.
        // Local runtimes are eligible exactly like cloud providers; the gateway
        // still applies local concurrency admission and normal failover.
        let (provider_id, model) = (resolved_provider, resolved_model);
        // Per-model effort wins, then the message, the room and the agent.
        // "none" turns reasoning off; an explicit level turns it on.
        let codex_route = provider_id.as_deref().and_then(|id|state.store.get("provider",id))
            .is_some_and(|provider|provider["kind"] == "codex");
        let effort_choice=rotated_effort
            .or_else(||optional(input,"reasoningEffort"))
            .or_else(||optional(&room,"reasoningEffort"))
            .or_else(||optional(&agent,"reasoningEffort"))
            .or_else(||provider_id.as_deref().and_then(|id|state.store.get("provider",id)).and_then(|provider|optional(&provider,"reasoningEffort")))
            // Codex's default is model-specific. Do not convert a greeting's
            // heuristic `none` into an unsupported effort on reasoning models.
            .or_else(||(!codex_route).then(||auto_reasoning_effort(owner_text)));
        let reasoning_effort=effort_choice.clone().filter(|effort|codex_route || effort != "none");
        let thinking=input["thinking"].as_bool()
            .or_else(||room["thinking"].as_bool())
            .or_else(||agent["thinking"].as_bool())
            .or_else(||effort_choice.as_deref().map(|effort| effort != "none"))
            .or_else(||(!codex_route).then_some(false));
        let reader=crate::engine::room_state::unread_for(
            state,room_id,session_id,&agent_id)?;
        let mut request=TurnRequest {direct_chat:false,
            session_id:Some(session_id.to_string()),request_id:None,input:owner_text.to_string(),agent_id:Some(agent_id.clone()),provider_id,model,attachment_ids: Vec::new(),tools_enabled:room["toolsEnabled"].as_bool().unwrap_or(false),tools_execution_enabled:true,thinking,auto_learning:room["autoLearning"].as_bool().unwrap_or(true),reasoning_effort,reasoning_language: (input["reasoningLanguage"].as_str().unwrap_or("ko") == "ko").then(|| "ko".into()),
            task_id:Some(task_id.clone()),parent_task_id:None,task_revision:task_revision.clone(),verification_owner:Some("self".into()),
            // The invariant room identity, participants, topic, and authority
            // are supplied by room_context::core. The current owner message,
            // source ID, and external speaker attribution stay in history.
            system_override:None,
        };
        if let Some(note) = crate::engine::debate::guidance(&conflict_resolution) {
            request.system_override = Some(note.to_string());
        }
        // The reader watermark is used for durable per-participant acknowledgement
        // after the turn. Unread source references are already represented by
        // the room context's attributed transcript/peer index. Injecting the
        // *moving* unread window as a system override changes the bot's
        // instructions on every round and defeats prefix reuse after restart.
        let message = match run_room_quiet(state, &request, source_id).await {
            Ok(Some(message)) => message,
            Ok(None) => return Err(format!("{name}의 응답이 비어 있습니다.")),
            Err(_) if stopped(context, session_id, generation) => {
                return Ok(finish(
                    context,
                    room_id,
                    session_id,
                    "cancelled",
                    &outputs,
                    stop_reason(context,session_id,generation).unwrap_or("session_cancelled"),
                    None,
                ))
            }
            Err(error) if error.contains("빈 응답") => {
                if let Some(assignment)=work_assignment.as_ref(){
                    crate::engine::room_workboard::skip(state,assignment,"empty")?;
                }
                context.emit(json!({"type":"silence","roomId":room_id,"agentId":agent_id,"reason":"empty"}));
                checkpoint(
                    context, session_id, generation, source_id, &mode, &roster, &initial, &pending,
                    &outputs, budget, false, None,
                )?;
                continue;
            }
            Err(error) => return Err(error),
        };
        let content = message["content"].as_str().unwrap_or("");
        if is_silence(content) {
            if let Some(assignment)=work_assignment.as_ref(){
                crate::engine::room_workboard::skip(state,assignment,"pass")?;
            }
            context.emit(json!({"type":"silence","roomId":room_id,"agentId":agent_id,"reason":"pass"}));
            checkpoint(
                context, session_id, generation, source_id, &mode, &roster, &initial, &pending,
                &outputs, budget, false, None,
            )?;
            continue;
        }
        let evidence_count = message_evidence_count(state, &message);
        // Message usage is the provider's reported per-speaker total when
        // available. Execution counters also include auxiliary planner and
        // review calls; neither count is inferred from a generated answer.
        let completed_execution=message["metadata"]["executionId"].as_str()
            .and_then(|id|state.store.get("execution",id))
            .unwrap_or(Value::Null);
        outputs.push(json!({
            "agentId":agent_id,
            "name":name,
            "content":content,
            "messageId":message["id"],
            "executionId":message["metadata"]["executionId"],
            "usage":message["metadata"]["usage"],
            "modelCalls":completed_execution["modelCalls"],
            "toolCallCount":completed_execution["toolCalls"],
            "inputContext":completed_execution["inputContext"]["wireRounds"]
                .as_array().and_then(|rounds|rounds.first()).cloned().unwrap_or(Value::Null),
            "evidenceCount":evidence_count,
            "toolCalls":message["tool_calls"].clone(),
        }));
        // Persist a task-scoped observation after the assistant message
        // is committed, before room checkpoint acknowledges the speaker.
        // Ordinary discussion rooms keep their historical transcript flow.
        if let Some(assignment)=work_assignment.as_ref(){
            let observed=crate::engine::room_workboard::record(
                state,room_id,session_id,source_id,&task_id,
                &agent_id,outputs.len(),&message,
            )?;
            crate::engine::room_workboard::complete(state,assignment,&observed)?;
        }
        crate::engine::room_state::acknowledge_read(
            state,room_id,session_id,&agent_id,source_id,
            message["id"].as_str().ok_or("응답 메시지 ID가 없습니다.")?,
            reader.through,
        )?;
        let resolution =
            crate::engine::debate::resolve(owner_text, &outputs, &conflict_strategy);
        if resolution["conflict"].as_bool() == Some(true)
            && resolution["revision"].as_str() != conflict_resolution["revision"].as_str()
        {
            if let Some(supported) =
                crate::engine::debate::supported_outcome(&resolution)
            {
                crate::engine::algorithms::record_domain_outcome(
                    state,
                    None,
                    "multiagent.conflict",
                    &conflict_strategy,
                    supported,
                );
            }
            state.store.audit(
                "room.conflict_resolved",
                json!({
                    "roomId": room_id,
                    "turns": outputs.len(),
                    "strategy": resolution["strategy"],
                    "outcome": resolution["outcome"],
                    "winnerAgentId": resolution["winner"]["agentId"],
                }),
            );
            conflict_resolution = resolution.clone();
            patch_room(
                state,
                room_id,
                json!({"conflictResolution": conflict_resolution.clone()}),
            );
        }
        if let Some(index) = roster.iter().position(|id| id == &agent_id) {
            patch_room(
                state,
                room_id,
                json!({"nextSpeakerIndex":(index+1)%roster.len()}),
            );
        }
        let mentions = mentioned_agents(state, &roster, content).unwrap_or_default();
        let spoken = outputs
            .iter()
            .filter_map(|output| output["agentId"].as_str())
            .map(str::to_string)
            .collect::<HashSet<_>>();
        route_peers(
            &mut pending,
            &mentions,
            &agent_id,
            outputs.len(),
            budget,
            if mode == "discussion" {
                Some(&spoken)
            } else {
                None
            },
        );
        if token_budget > 0 {
            let used = state.store.session_usage(session_id)["totalTokens"]
                .as_u64()
                .unwrap_or(0)
                .saturating_sub(usage_baseline);
            if used >= token_budget {
                checkpoint(
                    context, session_id, generation, source_id, &mode, &roster, &initial,
                    &pending, &outputs, budget, false, None,
                )?;
                return Ok(finish(
                    context,
                    room_id,
                    session_id,
                    "completed",
                    &outputs,
                    "token_budget",
                    None,
                ));
            }
        }
        checkpoint(
            context, session_id, generation, source_id, &mode, &roster, &initial, &pending,
            &outputs, budget, false, None,
        )?;
    }
    if stopped(context, session_id, generation) {
        return Ok(finish(
            context,
            room_id,
            session_id,
            "cancelled",
            &outputs,
            stop_reason(context,session_id,generation).unwrap_or("session_cancelled"),
            None,
        ));
    }
    let reason = if outputs.len() >= budget {
        "turn_limit"
    } else if outputs.is_empty() {
        "waiting_for_mention"
    } else {
        "waiting_for_owner"
    };
    Ok(finish(
        context,
        room_id,
        session_id,
        "completed",
        &outputs,
        reason,
        None,
    ))
}

fn owner_message(
    context: &JobContext,
    input: &Value,
    session_id: &str,
    room_id: &str,
) -> Result<Value, String> {
    let messages = context.state.store.messages(session_id);
    if let Some(id) = input
        .get("messageId")
        .or_else(|| input.get("sourceMessageId"))
        .and_then(Value::as_str)
    {
        return messages
            .into_iter()
            .find(|message| message["id"].as_str() == Some(id) && is_owner(message))
            .ok_or_else(|| "실제 주인 메시지가 대화방 기록에 없습니다.".into());
    }
    // /run compatibility is still one owner utterance, never one per agent.
    let text = input
        .get("goal")
        .or_else(|| input.get("input"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if text.is_empty() {
        return Err("대화 메시지가 비어 있습니다.".into());
    }
    let id = format!("room-owner:{}", context.job_id);
    if let Some(message) = messages
        .into_iter()
        .find(|message| message["id"].as_str() == Some(id.as_str()))
    {
        return Ok(message);
    }
    let saved=context.state.store.write_message(&json!({"id":id,"session_id":session_id,"role":"user","content":text,"agent_id":null,"metadata":{"authorKind":"owner","roomId":room_id}})).ok_or("주인 메시지를 저장할 수 없습니다.")?;
    context.state.bus.message(&saved);
    Ok(saved)
}
fn is_owner(message: &Value) -> bool {
    message["role"] == "user"
        && message["agent_id"].is_null()
        && message
            .pointer("/metadata/authorKind")
            .and_then(Value::as_str)
            == Some("owner")
}
/// Evidence is not only the acceptance-check list. A tool operation that
/// actually completed is evidence too; the previous count ignored it, so a
/// successful tool-using turn could display `evidenceCount: 0`.
fn message_evidence_count(state: &AppState, message: &Value) -> usize {
    let recorded = message["metadata"]["evidence"]
        .as_array()
        .map(|items| items.len())
        .unwrap_or(0);
    let operations = message["metadata"]["executionId"]
        .as_str()
        .and_then(|run_id| state.store.execution_operations(run_id).ok())
        .map(|items| {
            items
                .iter()
                .filter(|operation| operation["status"].as_str() == Some("completed"))
                .count()
        })
        .unwrap_or(0);
    recorded + operations
}

fn newer_owner_message(state: &AppState, session_id: &str, source_id: &str) -> Option<String> {
    let messages = state.store.messages(session_id);
    let index = messages
        .iter()
        .position(|message| message["id"].as_str() == Some(source_id))?;
    messages[index + 1..]
        .iter()
        .rev()
        .find(|message| is_owner(message))
        .and_then(|message| message["id"].as_str())
        .map(str::to_string)
}
fn job_cancelled(context: &JobContext) -> bool {
    context
        .state
        .store
        .get("job", &context.job_id)
        .is_some_and(|job| {
            job["cancelRequested"].as_bool() == Some(true) || job["status"] == "cancelled"
        })
}
/// Stable diagnostic reason. A job stop, changed SQLite epoch, and a local
/// cancellation notification have different recovery implications.
fn stop_reason(context: &JobContext, session_id: &str, generation: u64)
    -> Option<&'static str>
{
    if job_cancelled(context) { return Some("job_cancel_requested"); }
    if context.state.cancellation_generation(session_id)!=generation {
        return Some("session_epoch_changed");
    }
    if context.state.is_cancelled(session_id) {
        return Some("session_cancelled");
    }
    None
}
fn stopped(context: &JobContext, session_id: &str, generation: u64) -> bool {
    stop_reason(context,session_id,generation).is_some()
}
fn patch_room(state: &AppState, room_id: &str, patch: Value) {
    if let Some(saved) = state
        .store
        .patch_if("room", room_id, "id", &json!(room_id), &patch)
    {
        state.bus.record("room", &saved);
    }
}
#[allow(clippy::too_many_arguments)]
fn checkpoint(
    context: &JobContext,
    session_id: &str,
    generation: u64,
    source_id: &str,
    mode: &str,
    roster: &[String],
    initial: &[String],
    pending: &VecDeque<String>,
    outputs: &[Value],
    budget: usize,
    in_flight: bool,
    active: Option<&str>,
) -> Result<(), String> {
    // roomState is replaced atomically by each checkpoint. Carry the
    // original per-job usage baseline through ALL subsequent checkpoints.
    let usage_baseline=context.state.store.get("job",&context.job_id)
        .and_then(|job|job["roomState"]["tokenUsageBaseline"].as_u64())
        .unwrap_or_else(||context.state.store.session_usage(session_id)["totalTokens"]
            .as_u64().unwrap_or(0));
    context.checkpoint(json!({"sessionId":session_id,"stepIndex":outputs.len(),"stepTotal":budget,"roomState":{"sessionId":session_id,"generation":generation,"sourceMessageId":source_id,"routingMode":mode,"roster":roster,"initialSpeakers":initial,"pendingSpeakers":pending,"turnBudget":budget,"tokenUsageBaseline":usage_baseline,"outputs":outputs,"inFlight":in_flight,"activeAgentId":active}}))
}
fn finish(
    context: &JobContext,
    room_id: &str,
    session_id: &str,
    status: &str,
    outputs: &[Value],
    reason: &str,
    yielded: Option<&str>,
) -> Value {
    patch_room(
        &context.state,
        room_id,
        json!({"status":if status=="cancelled" {"paused"} else {"idle"},"activeAgentId":null,"pauseReason":reason,"yieldedToMessageId":yielded}),
    );
    context.state.store.audit("room.exchange_finished",json!({
        "roomId":room_id,"sessionId":session_id,"jobId":context.job_id,
        "turns":outputs.len(),"status":status,"reason":reason,
        "generation":context.state.cancellation_generation(session_id),
        "cancelRequested":job_cancelled(context)
    }));
    context.emit(json!({"type":"room_finished","roomId":room_id,"sessionId":session_id,"status":status,"reason":reason}));
    // One completed exchange is one learning unit. The job is content-
    // addressed by its last message, so retries and duplicate completion do
    // not create duplicate knowledge.
    if status == "completed" && !outputs.is_empty() {
        let learning_enabled = context
            .state
            .store
            .get("room", room_id)
            .is_some_and(|room| room["autoLearning"].as_bool() != Some(false));
        if learning_enabled {
            // Snapshot the exchange this job actually completed. Re-reading the
            // session tail could pick up a newer owner message that arrived
            // while the final turn was finishing.
            if let Some(through) = outputs
                .iter()
                .rev()
                .find_map(|output| output["messageId"].as_str())
            {
                crate::engine::room_knowledge::enqueue_automatic(
                    &context.state,
                    room_id,
                    session_id,
                    through,
                );
            }
        }
    }
    json!({"roomId":room_id,"sessionId":session_id,"turns":outputs.len(),"status":status,"outputs":outputs,"pauseReason":reason,"yieldedToMessageId":yielded})
}
fn route_peers(
    pending: &mut VecDeque<String>,
    mentions: &[String],
    speaker: &str,
    used: usize,
    budget: usize,
    discussion_spoken: Option<&HashSet<String>>,
) {
    for id in mentions.iter().rev().filter(|id| id.as_str() != speaker) {
        // Discussion gives the selected cohort a chance to speak before an
        // already-spoken pair can consume all remaining turns mentioning one
        // another. Mentioned/round-robin conversations remain freely directed.
        if discussion_spoken.is_some_and(|spoken| {
            spoken.contains(id) && pending.iter().any(|next| !spoken.contains(next))
        }) {
            continue;
        }
        if let Some(index) = pending.iter().position(|next| next == id) {
            pending.remove(index);
            pending.push_front(id.clone());
        } else if used + pending.len() < budget {
            pending.push_front(id.clone());
        }
    }
}
fn routing_mode(state: &AppState, room: &Value) -> Result<String, String> {
    if let Some(mode) = room["routingMode"].as_str() {
        if matches!(mode, "discussion" | "round_robin" | "mentioned") {
            return Ok(mode.to_string());
        }
        return Err("응답 방식은 discussion, round_robin, mentioned 중 하나여야 합니다.".into());
    }
    let kernel = crate::engine::algorithms::choose_for_agent(
        state, "multiagent.roles", None, None,
    )
    .and_then(|algorithm| algorithm["implementation"].as_str().map(str::to_string))
    .unwrap_or_else(|| "discussion".into());
    Ok(match kernel.as_str() {
        "round-robin" => "round_robin".into(),
        "mentioned" => "mentioned".into(),
        _ => "discussion".into(),
    })
}
fn id_list(value: &Value) -> Result<Vec<String>, String> {
    let mut seen = HashSet::new();
    let ids = id_list_with_repeats(value)?
        .into_iter()
        .filter(|id| seen.insert(id.clone()))
        .collect::<Vec<_>>();
    if ids.len() > MAX_PARTICIPANTS {
        return Err("대화방 참가자는 최대 32명입니다.".into());
    }
    Ok(ids)
}
fn id_list_with_repeats(value: &Value) -> Result<Vec<String>, String> {
    value
        .as_array()
        .ok_or("참가 에이전트 ID 배열이 필요합니다.")?
        .iter()
        .map(|value| {
            value
                .as_str()
                .filter(|id| !id.trim().is_empty())
                .map(str::to_string)
                .ok_or_else(|| "유효하지 않은 에이전트 ID입니다.".into())
        })
        .collect()
}
fn participants(room: &Value) -> Result<Vec<String>, String> {
    let ids = id_list(&room["agentIds"])?;
    if ids.is_empty() {
        return Err("대화방 참가자가 없습니다.".into());
    }
    Ok(ids)
}
fn optional(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Mentions inside examples/code/quoted blocks are data, not routing commands.
fn mention_text(input: &str) -> String {
    // A fence opens with N backticks or tildes and closes only with the same
    // character and at least N markers. Nested shorter markers are data.
    let mut fence: Option<(char, usize)> = None;
    let mut out = String::new();
    for line in input.lines() {
        let trimmed = line.trim_start();
        if let Some(marker) = trimmed.chars().next().filter(|c| matches!(c, '`' | '~')) {
            let count = trimmed.bytes().take_while(|byte| *byte == marker as u8).count();
            if count >= 3 {
                match fence {
                    None => {
                        fence = Some((marker, count));
                        out.push('\n');
                        continue;
                    }
                    Some((opened, minimum))
                        if opened == marker && count >= minimum && trimmed[count..].trim().is_empty() =>
                    {
                        fence = None;
                        out.push('\n');
                        continue;
                    }
                    _ => {}
                }
            }
        }
        if fence.is_some() || trimmed.starts_with('>') {
            out.push('\n');
            continue;
        }
        let mut inline = false;
        let mut url = false;
        for (position, ch) in line.char_indices() {
            let rest = &line[position..];
            if !inline
                && (rest.get(..7).is_some_and(|scheme| scheme.eq_ignore_ascii_case("http://"))
                    || rest.get(..8).is_some_and(|scheme| scheme.eq_ignore_ascii_case("https://")))
            {
                url = true;
            }
            // An @handle in a pasted HTTP(S) address is reference data, not
            // a request to activate a same-named Room participant.
            if url && !ch.is_whitespace() && !matches!(ch, ')' | ']' | '>' | '"') {
                out.push(' ');
                continue;
            }
            url = false;
            if ch == '`' {
                inline = !inline;
                out.push(' ');
            } else if inline {
                out.push(' ');
            } else {
                out.push(ch);
            }
        }
        out.push('\n');
    }
    out
}

fn mention_boundary(ch: Option<char>) -> bool {
    ch.is_none_or(|ch| !ch.is_alphanumeric() && !matches!(ch, '_' | '-' | '.'))
}
fn bare_mention_follows(after: &str, directive: bool) -> bool {
    // Vocatives address a participant even without a trailing imperative.
    // Subject/coordinator particles alone are descriptive; permit them only
    // when the owner's sentence actually requests those agents to act.
    const ADDRESS: &[&str] = &["야", "아", "님께", "님", "씨", "에게", "한테", "께"];
    const CLAUSE: &[&str] = &["와", "과", "랑", "이랑", "가", "이", "은", "는"];
    let addressed=ADDRESS.iter().any(|particle| after.strip_prefix(particle)
        .is_some_and(|suffix|mention_boundary(suffix.chars().next())));
    addressed || directive && CLAUSE.iter().any(|particle|after.strip_prefix(particle)
        .is_some_and(|suffix|mention_boundary(suffix.chars().next())))
}
fn explicit_agent_directive(text:&str)->bool{
    // Restrict bare-name subject lists to actual owner requests, not ordinary
    // descriptions of who said/did something. @id works without this check.
    let ending=text.trim_end_matches(|ch:char|ch.is_whitespace()
        || matches!(ch,'.'|'!'|'?'|'。'|'！'|'？'));
    ["해","해줘","해봐","해라","해줘요","해주세요","해 주세요",
      "말해","말해줘","답해","답해줘","읽어","읽어줘","찾아","찾아줘",
      "해주시겠어요","확인해주세요","확인해줘","정리해줘","검토해줘",
      "진행해줘","작성해줘","조사해줘"]
        .iter().any(|verb|ending.ends_with(verb))
}
/// Optional silence: a member may decline its turn instead of speaking.
fn is_silence(content: &str) -> bool {
    let text = content.trim();
    text.is_empty()
        || text.eq_ignore_ascii_case("(pass)")
        || text.eq_ignore_ascii_case("(pass.)")
        || text.eq_ignore_ascii_case("pass")
        || text.eq_ignore_ascii_case("pass.")
}

fn mentioned_agents(
    state: &AppState,
    roster: &[String],
    input: &str,
) -> Result<Vec<String>, String> {
    let text = mention_text(input).to_lowercase();
    let directive=explicit_agent_directive(&text);
    let mut matches = Vec::new();
    for (order, id) in roster.iter().enumerate() {
        let agent = state
            .store
            .get("agent", id)
            .ok_or_else(|| format!("참가 에이전트를 찾을 수 없습니다: {id}"))?;
        let mut aliases = vec![id.to_lowercase()];
        if let Some(name) = agent["name"]
            .as_str()
            .filter(|name| !name.trim().is_empty())
        {
            aliases.push(name.to_lowercase());
        }
        // Owner-configured aliases are first-class routing identities too.
        // Do not infer names from private prompts or peer transcript text.
        if let Some(names)=agent["aliases"].as_array(){
            for name in names.iter().filter_map(Value::as_str)
                .filter(|name|!name.trim().is_empty() && name.chars().count()<=80){
                aliases.push(name.to_lowercase());
            }
        }
        aliases.sort();
        aliases.dedup();
        for alias in aliases {
            let marker = format!("@{alias}");
            for (position, _) in text.match_indices(&marker) {
                if mention_boundary(text[..position].chars().next_back())
                    && mention_boundary(text[position + marker.len()..].chars().next())
                {
                    matches.push((position, marker.len(), order, id.clone(), alias.clone()));
                }
            }
            // Owners often address a participant naturally ("샤르티아야",
            // "샤르티아에게") instead of typing an @mention. Treat a bare name
            // followed by a vocative/particle or a boundary as a direct target.
            if alias.chars().count() < 2 {
                continue;
            }
            for (position, _) in text.match_indices(&alias) {
                if text[..position].ends_with('@') {
                    continue;
                }
                if !mention_boundary(text[..position].chars().next_back()) {
                    continue;
                }
                let after = &text[position + alias.len()..];
                if bare_mention_follows(after,directive) {
                    matches.push((position, alias.len(), order, id.clone(), alias.clone()));
                }
            }
        }
    }
    matches.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then_with(|| b.1.cmp(&a.1))
            .then_with(|| a.2.cmp(&b.2))
    });
    for pair in matches.windows(2) {
        if pair[0].0 == pair[1].0 && pair[0].4 == pair[1].4 && pair[0].3 != pair[1].3 {
            return Err("같은 이름의 참가자가 있습니다. @에이전트ID로 지정하세요.".into());
        }
    }
    let mut seen = HashSet::new();
    Ok(matches
        .into_iter()
        .filter_map(|(_, _, _, id, _)| seen.insert(id.clone()).then_some(id))
        .collect())
}

#[cfg(test)]
mod pool_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn participant_model_pool_is_deduplicated_and_generic() {
        let dir = std::env::temp_dir().join(format!("dominion-pool-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = crate::config::Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let state = crate::state::AppState::new(
            config,
            crate::db::Store::open(&dir.join("db.sqlite")).unwrap(),
            crate::vault::Vault::open(&dir.join("keys.json")),
        );
        state.store.put("agent", &json!({"id":"a","providerId":"p1","model":"m1"}));
        state.store.put("agent", &json!({"id":"b","providerId":"p1","model":"m1"}));
        state.store.put("agent", &json!({"id":"c","providerId":"p2","model":"m2"}));
        let pool = model_pool(&state, &json!({"agentIds":["a","b","c"]}));
        assert_eq!(pool, vec![("p1".into(),"m1".into(),None),("p2".into(),"m2".into(),None)]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn explicit_room_models_override_agent_defaults() {
        let dir = std::env::temp_dir().join(format!("dominion-pool-explicit-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = crate::config::Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let state = crate::state::AppState::new(
            config,
            crate::db::Store::open(&dir.join("db.sqlite")).unwrap(),
            crate::vault::Vault::open(&dir.join("keys.json")),
        );
        state.store.put("provider", &json!({"id":"cloud","kind":"openai","baseUrl":"https://api.example.com/v1"}));
        state.store.put("provider", &json!({"id":"local","kind":"ollama","baseUrl":"http://127.0.0.1:11434/v1"}));
        state.store.put("agent", &json!({"id":"a","providerId":"cloud","model":"agent-default"}));
        let room = json!({
            "agentIds":["a"],
            "models":[
                {"providerId":"cloud","model":"picked-fast","reasoningEffort":"low"},
                {"providerId":"cloud","model":"picked-fast"},
                {"providerId":"local","model":"picked-local"}
            ]
        });
        let pool = model_pool(&state, &room);
        assert_eq!(
            pool,
            vec![
                ("cloud".into(), "picked-fast".into(), Some("low".into())),
                ("local".into(), "picked-local".into(), None)
            ]
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn participant_model_pool_keeps_local_and_cloud_routes() {
        let dir = std::env::temp_dir().join(format!("dominion-pool-local-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = crate::config::Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let state = crate::state::AppState::new(
            config,
            crate::db::Store::open(&dir.join("db.sqlite")).unwrap(),
            crate::vault::Vault::open(&dir.join("keys.json")),
        );
        state.store.put("provider", &json!({"id":"local","kind":"mlx","local":true,"baseUrl":"http://127.0.0.1:1234/v1"}));
        state.store.put("provider", &json!({"id":"cloud","kind":"openai","baseUrl":"https://api.example.com/v1"}));
        state.store.put("agent", &json!({"id":"a","providerId":"local","model":"mlx-7b"}));
        state.store.put("agent", &json!({"id":"b","providerId":"cloud","model":"free-model"}));
        let pool = model_pool(&state, &json!({"agentIds":["a","b"]}));
        assert_eq!(
            pool,
            vec![
                ("local".into(), "mlx-7b".into(), None),
                ("cloud".into(), "free-model".into(), None)
            ]
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        path::PathBuf,
        sync::{Arc, Mutex},
    };

    fn test_state() -> (AppState, PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("dominion-shared-room-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = crate::config::Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let store = crate::db::Store::open(&config.db_path()).unwrap();
        let vault = crate::vault::Vault::open(&config.key_path());
        let state = AppState::new(config, store, vault);
        for (id, name) in [("a", "가람"), ("b", "나루"), ("c", "다온")] {
            state.store.put(
                "agent",
                &json!({"id":id,"name":name,"providerId":"p","model":"test"}),
            );
        }
        state.store.put("room",&json!({"id":"r","name":"우리 대화방","agentIds":["a","b","c"],"routingMode":"discussion","rounds":1,"maxTurns":6,"toolsEnabled":false,"autoLearning":false}));
        (state, dir)
    }
    fn room(state: &AppState) -> Value {
        state.store.get("room", "r").unwrap()
    }

    #[test]
    fn natural_language_vocative_targets_only_the_named_agent() {
        let (state, dir) = test_state();
        for (id, name) in [("s", "샤르티아"), ("a", "아인즈"), ("b", "알베도")] {
            state
                .store
                .put("agent", &json!({"id":id,"name":name}));
        }
        let roster = vec!["s".to_string(), "a".to_string(), "b".to_string()];
        assert_eq!(
            mentioned_agents(&state, &roster, "샤르티아야 니가 결과를 가져와봐라").unwrap(),
            vec!["s".to_string()]
        );
        assert_eq!(
            mentioned_agents(&state, &roster, "샤르티아에게 시켜").unwrap(),
            vec!["s".to_string()]
        );
        assert_eq!(
            mentioned_agents(&state, &roster, "아인즈와 알베도가 확인해").unwrap(),
            vec!["a".to_string(), "b".to_string()]
        );
        assert_eq!(
            mentioned_agents(&state, &roster, "아인즈와 알베도가 등장했다고 들었어").unwrap(),
            Vec::<String>::new(),
            "descriptive sentences must not route agents"
        );
        assert_eq!(
            mentioned_agents(&state, &roster, "@아인즈 와 @알베도, 의견 부탁드립니다").unwrap(),
            vec!["a".to_string(), "b".to_string()],
            "explicit mentions work regardless of bare-name clause heuristics"
        );
        assert_eq!(
            mentioned_agents(&state, &roster, "`아인즈야` 예시를 보여줘").unwrap(),
            Vec::<String>::new(),
            "inline code examples are never a routing command"
        );
        state.store.put("agent",&json!({"id":"s","name":"샤르티아",
            "aliases":["샤르","대장"]}));
        assert_eq!(mentioned_agents(&state,&roster,"@대장 결과 알려줘").unwrap(),
            vec!["s".to_string()]);
        assert_eq!(mentioned_agents(&state,&roster,"샤르야 진행해줘").unwrap(),
            vec!["s".to_string()]);
        assert!(mentioned_agents(&state,&roster,"대장은 지난번에 조사했어").unwrap().is_empty(),
            "descriptive references to an alias must not route the agent");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn room_evidence_count_includes_completed_tool_operations() {
        let (state, dir) = test_state();
        state
            .store
            .put("execution", &json!({"id":"run-1","sessionId":"s","agentId":"a"}));
        state.store.put(
            "execution_operation",
            &json!({"id":"op-ok","runId":"run-1","tool":"workspace_list","status":"completed"}),
        );
        state.store.put(
            "execution_operation",
            &json!({"id":"op-fail","runId":"run-1","tool":"workspace_write","status":"failed"}),
        );
        assert_eq!(
            message_evidence_count(&state, &json!({"metadata":{"executionId":"run-1"}})),
            1
        );
        assert_eq!(
            message_evidence_count(
                &state,
                &json!({"metadata":{"evidence":[{"id":"e1"},{"id":"e2"}]}})
            ),
            2
        );
        assert_eq!(
            message_evidence_count(
                &state,
                &json!({"metadata":{"executionId":"run-1","evidence":[{"id":"e1"}]}})
            ),
            2
        );
        let _ = std::fs::remove_dir_all(dir);
    }
    fn save_owner_job(state: &AppState, id: &str, text: &str) -> Value {
        let session = ensure_room_session(state, "r").unwrap();
        let sid = session["id"].as_str().unwrap();
        let message=state.store.write_message(&json!({"id":format!("{id}:owner"),"session_id":sid,"role":"user","content":text,"metadata":{"authorKind":"owner","roomId":"r"}})).unwrap();
        state.store.put("job",&json!({"id":id,"kind":"room","status":"running","input":{"roomId":"r","sessionId":sid,"messageId":message["id"],"sourceMessageId":message["id"],"input":text,"generation":state.cancellation_generation(sid),"roomSnapshot":room(state),"explicitAgentIds":false}})).unwrap()
    }
    type Callback = Arc<dyn Fn(usize, &Value) + Send + Sync>;
    async fn provider(
        state: &AppState,
        deltas: Vec<Value>,
        callback: Option<Callback>,
    ) -> (tokio::task::JoinHandle<()>, Arc<Mutex<Vec<Value>>>) {
        use axum::{extract::State, routing::post, Json, Router};
        #[derive(Clone)]
        struct Mock {
            deltas: Arc<Vec<Value>>,
            bodies: Arc<Mutex<Vec<Value>>>,
            callback: Option<Callback>,
        }
        async fn handle(
            State(mock): State<Mock>,
            Json(body): Json<Value>,
        ) -> ([(&'static str, &'static str); 1], String) {
            let index = {
                let mut bodies = mock.bodies.lock().unwrap();
                let index = bodies.len();
                bodies.push(body.clone());
                index
            };
            if let Some(callback) = &mock.callback {
                callback(index, &body);
            }
            let delta = mock
                .deltas
                .get(index)
                .cloned()
                .unwrap_or(json!({"content":"좋아, 주인 이야기를 기다릴게."}));
            let stream = format!(
                "data: {}\n\ndata: [DONE]\n\n",
                json!({"choices":[{"delta":delta}]})
            );
            ([("content-type", "text/event-stream")], stream)
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        state.store.put("provider",&json!({"id":"p","name":"mock","baseUrl":format!("http://127.0.0.1:{port}/v1"),"model":"test","enabled":true,"toolMode":"native"}));
        let bodies = Arc::new(Mutex::new(Vec::new()));
        let router = Router::new()
            .route("/v1/chat/completions", post(handle))
            .with_state(Mock {
                deltas: Arc::new(deltas),
                bodies: bodies.clone(),
                callback,
            });
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        (server, bodies)
    }
    fn text_delta(text: &str) -> Value {
        json!({"content":text})
    }
    fn run_context(state: &AppState, id: &str) -> JobContext {
        JobContext::new(state.clone(), id.to_string())
    }

    #[test]
    fn room_session_is_stable_and_cannot_adopt_private_or_other_room_chat() {
        let (state, dir) = test_state();
        let first = ensure_room_session(&state, "r").unwrap();
        let second = ensure_room_session(&state, "r").unwrap();
        assert_eq!(first["id"], second["id"]);
        assert_eq!(state.store.count("session"), 1);
        state
            .store
            .put("session", &json!({"id":"private","title":"private"}));
        state.store.patch_if(
            "room",
            "r",
            "id",
            &json!("r"),
            &json!({"sessionId":"private"}),
        );
        assert!(ensure_room_session(&state, "r").is_err());
        state
            .store
            .put("session", &json!({"id":"other-room","roomId":"other"}));
        state.store.patch_if(
            "room",
            "r",
            "id",
            &json!("r"),
            &json!({"sessionId":"other-room"}),
        );
        assert!(ensure_room_session(&state, "r").is_err());
        drop(state); std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn short_casual_requests_use_a_small_speaker_window() {
        let (state, dir) = test_state();
        for (id, name) in [("d", "라온"), ("e", "마루")] {
            state.store.put(
                "agent",
                &json!({"id":id,"name":name,"providerId":"p","model":"test"}),
            );
        }
        state.store.patch_if(
            "room",
            "r",
            "id",
            &json!("r"),
            &json!({"agentIds":["a","b","c","d","e"],"maxTurns":0}),
        );
        let config = room(&state);
        let casual = plan_speakers(&state, &config, "안녕", None).unwrap();
        assert_eq!(casual.len(), 2);
        assert!(casual.contains(&"a".to_string()));
        let everyone = plan_speakers(&state, &config, "다 같이 얘기하자", None).unwrap();
        assert_eq!(everyone, vec!["a", "b", "c", "d", "e"]);
        let mentioned = plan_speakers(&state, &config, "@나루 답해", None).unwrap();
        assert_eq!(mentioned, vec!["b"]);
        drop(state); std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn pool_slots_are_stable_and_distinct_for_participants() {
        let roster = vec!["a".to_string(), "b".to_string()];
        assert_eq!(pool_slot(&roster, "a", 2), 0);
        assert_eq!(pool_slot(&roster, "b", 2), 1);
        assert_eq!(pool_slot(&roster, "a", 2), pool_slot(&roster, "a", 2));
    }

    #[test]
    fn routing_modes_respect_owner_selection_ids_and_real_mentions() {
        let (state, dir) = test_state();
        let mut config = room(&state);
        assert_eq!(
            plan_speakers(&state, &config, "@나루 너부터", None).unwrap(),
            vec!["b"]
        );
        config["routingMode"] = json!("round_robin");
        config["nextSpeakerIndex"] = json!(2);
        assert_eq!(
            plan_speakers(&state, &config, "안녕", None).unwrap(),
            vec!["c"]
        );
        config["routingMode"] = json!("mentioned");
        assert!(plan_speakers(&state, &config, "예시로 나루 13, 다온 77을 적어주세요", None)
            .unwrap().is_empty(),"quoted/example names are data, not direct address");
        assert_eq!(plan_speakers(&state, &config, "나루야 답해", None).unwrap(),vec!["b"]);
        assert_eq!(plan_speakers(&state, &config, "가람에게 질문할게", None).unwrap(),vec!["a"]);
        assert!(plan_speakers(&state, &config, "안녕", None)
            .unwrap()
            .is_empty());
        assert!(plan_speakers(&state, &config,
            "자료 링크는 https://example.test/team/@나루 이니까 읽어줘", None)
            .unwrap().is_empty(), "HTTP links are references, never mentions");
        assert_eq!(plan_speakers(&state, &config,
            "````md\n@나루 지금 답해\n```\n@가람 지금 답해\n````\n@다온 답해",
            None).unwrap(), vec!["c"], "nested shorter code fences remain data");
        assert_eq!(
            plan_speakers(&state, &config, "@b 그리고 @가람", None).unwrap(),
            vec!["b", "a"]
        );
        assert_eq!(
            plan_speakers(&state, &config, "안녕", Some(&json!(["c", "c", "a"]))).unwrap(),
            vec!["c", "a"]
        );
        assert!(plan_speakers(&state, &config, "안녕", Some(&json!(["outsider"]))).is_err());
        assert!(plan_speakers(
            &state,
            &config,
            "예시 `@가람`\n> @나루\n```text\n@다온\n```\nmail@b @가람이야",
            None
        )
        .unwrap()
        .is_empty());
        drop(state); std::fs::remove_dir_all(dir).unwrap();
    }
    #[tokio::test]
    async fn shared_conversation_stores_owner_once_and_every_agent_sees_previous_peers() {
        let (state, dir) = test_state();
        let (server, bodies) = provider(
            &state,
            vec![
                text_delta("첫 번째 관점"),
                text_delta("첫 번째 의견에 답할게"),
                text_delta("둘의 의견을 보고 내 생각"),
            ],
            None,
        )
        .await;
        let job = save_owner_job(&state, "j", "다 같이 오늘 편하게 얘기하자");
        let context = run_context(&state, "j");
        let result = run(&context, &job).await.unwrap();
        assert_eq!(result["status"], "completed");
        assert_eq!(result["turns"], 3);
        let messages = state.store.messages("room:r");
        assert_eq!(
            messages
                .iter()
                .filter(|message| message["role"] == "user")
                .count(),
            1
        );
        let speakers = messages
            .iter()
            .filter(|message| message["role"] == "assistant")
            .map(|message| message["agent_id"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(speakers, vec!["a", "b", "c"]);
        let requests = bodies.lock().unwrap();
        assert_eq!(requests.len(), 3);
        for body in requests.iter() {
            assert_eq!(
                body["messages"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|message| message["role"] == "user"
                        && message["content"]
                            .as_str()
                            .is_some_and(|content| content.ends_with("오늘 편하게 얘기하자")))
                    .count(),
                1
            );
        }
        assert!(requests[1]["messages"].to_string().contains("첫 번째 관점"));
        assert!(requests[2]["messages"].to_string().contains("첫 번째 관점"));
        assert!(requests[2]["messages"]
            .to_string()
            .contains("첫 번째 의견에 답할게"));
        assert_eq!(
            state.store.get("job", "j").unwrap()["roomState"]["inFlight"],
            false
        );
        drop(requests);
        server.abort();
        drop(context); drop(state); std::fs::remove_dir_all(dir).unwrap();
    }
    #[tokio::test]
    async fn mentioned_agents_can_reply_to_each_other_without_fabricated_owner_turns() {
        let (state, dir) = test_state();
        state.store.patch_if(
            "room",
            "r",
            "id",
            &json!("r"),
            &json!({"routingMode":"mentioned","maxTurns":5}),
        );
        let (server, _) = provider(
            &state,
            vec![
                text_delta("@나루 너는 어때?"),
                text_delta("@다온 같이 얘기해보자"),
                text_delta("내 생각은 이래"),
            ],
            None,
        )
        .await;
        let job = save_owner_job(&state, "j", "@가람 오늘 어땠어?");
        let result = run(&run_context(&state, "j"), &job).await.unwrap();
        assert_eq!(result["turns"], 3);
        assert_eq!(
            result["outputs"]
                .as_array()
                .unwrap()
                .iter()
                .map(|output| output["agentId"].as_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["a", "b", "c"]
        );
        assert_eq!(
            state
                .store
                .messages("room:r")
                .iter()
                .filter(|message| message["role"] == "user")
                .count(),
            1
        );
        server.abort();
        drop(state); std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn an_explicit_turn_cap_keeps_all_room_speakers() {
        let (state, dir) = test_state();
        let mut ids = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        for index in 4..=9 {
            let id = format!("a{index}");
            state.store.put(
                "agent",
                &json!({"id":id,"name":format!("참가자{index}"),"providerId":"p","model":"test"}),
            );
            ids.push(id);
        }
        let mut room = json!({
            "id":"r","agentIds":ids,"routingMode":"discussion","rounds":1,"maxTurns":9
        });
        assert_eq!(speaker_window(&state, &room, "다시 인사해봐", &ids).len(), 9);
        assert_eq!(speaker_window(&state, &room, "나머지도 말해라", &ids).len(), 9);
        room["maxTurns"] = json!(0);
        assert_eq!(speaker_window(&state, &room, "짧게 말해", &ids).len(), 2);
        drop(state); std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn discussion_follows_mentions_and_leaves_others_silent() {
        let (state, dir) = test_state();
        state.store.patch_if(
            "room",
            "r",
            "id",
            &json!("r"),
            &json!({"routingMode":"discussion","rounds":1,"maxTurns":3}),
        );
        let (server, bodies) = provider(&state, vec![text_delta("내 생각은 이래")], None).await;
        let job = save_owner_job(&state, "mention-cohort", "@나루 오늘 어땠어?");
        let result = run(&run_context(&state, "mention-cohort"), &job).await.unwrap();
        assert_eq!(result["turns"], 1);
        assert_eq!(result["outputs"][0]["agentId"], "b");
        assert_eq!(bodies.lock().unwrap().len(), 1);
        server.abort();
        drop(state); std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn pass_marker_is_silence_and_does_not_store_a_message() {
        let (state, dir) = test_state();
        state.store.patch_if(
            "room",
            "r",
            "id",
            &json!("r"),
            &json!({"routingMode":"discussion","rounds":1,"maxTurns":2}),
        );
        let (server, bodies) = provider(
            &state,
            vec![text_delta("(pass)"), text_delta("나는 말할게")],
            None,
        )
        .await;
        let job = save_owner_job(&state, "pass-silence", "둘이 얘기해봐");
        let result = run(&run_context(&state, "pass-silence"), &job).await.unwrap();
        assert_eq!(result["turns"], 1);
        let speakers = result["outputs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|output| output["agentId"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(speakers, vec!["b"]);
        assert_eq!(bodies.lock().unwrap().len(), 2);
        server.abort();
        drop(state); std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn owner_room_token_budget_keeps_its_baseline_after_checkpoint_replay() {
        let (state,dir)=test_state();
        save_owner_job(&state,"budget-replay","@가람 hello");
        let context=run_context(&state,"budget-replay");
        let roster=vec!["a".to_string()];
        let pending=VecDeque::from(roster.clone());
        checkpoint(&context,"room:r",0,"owner-1","discussion",
            &roster,&roster,&pending,&[],1,false,None).unwrap();
        let baseline=state.store.get("job","budget-replay").unwrap()["roomState"]["tokenUsageBaseline"].clone();
        state.store.audit("model.usage",json!({
            "sessionId":"room:r","providerId":"mock","model":"test",
            "promptTokens":800,"completionTokens":200,"totalTokens":1000,
            "estimated":false
        }));
        checkpoint(&context,"room:r",0,"owner-1","discussion",
            &roster,&roster,&pending,&[],1,false,None).unwrap();
        let persisted=state.store.get("job","budget-replay").unwrap();
        assert_eq!(persisted["roomState"]["tokenUsageBaseline"],baseline);
        let used=state.store.session_usage("room:r")["totalTokens"].as_u64().unwrap_or(0)
            .saturating_sub(baseline.as_u64().unwrap_or(0));
        assert_eq!(used,1000);
        drop(context);
        drop(state);
        let _=std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn peer_loops_are_bounded_and_owner_interjection_yields_at_boundary() {
        let (state, dir) = test_state();
        state.store.patch_if(
            "room",
            "r",
            "id",
            &json!("r"),
            &json!({"routingMode":"mentioned","maxTurns":3}),
        );
        let (server, _) = provider(
            &state,
            vec![
                text_delta("@나루 말해줘"),
                text_delta("@가람 너도"),
                text_delta("@나루 다시"),
            ],
            None,
        )
        .await;
        let job = save_owner_job(&state, "loop", "@가람 얘기해봐");
        let result = run(&run_context(&state, "loop"), &job).await.unwrap();
        assert_eq!(result["turns"], 3);
        assert_eq!(result["pauseReason"], "turn_limit");
        server.abort(); let _=server.await;
        let state_clone = state.clone();
        let callback: Callback = Arc::new(move |index, _| {
            if index == 0 {
                state_clone.store.write_message(&json!({"id":"new-owner","session_id":"room:r","role":"user","content":"잠깐, 새 주제로 바꿔","metadata":{"authorKind":"owner","roomId":"r"}}));
            }
        });
        state.store.patch_if(
            "room",
            "r",
            "id",
            &json!("r"),
            &json!({"routingMode":"discussion","maxTurns":6}),
        );
        let (server, _) = provider(&state, vec![text_delta("알겠어")], Some(callback)).await;
        let job = save_owner_job(&state, "interject", "이번 주제에 대해 얘기하자");
        let result = run(&run_context(&state, "interject"), &job).await.unwrap();
        assert_eq!(result["turns"], 1);
        assert_eq!(result["yieldedToMessageId"], "new-owner");
        server.abort(); let _=server.await;
        drop(state);
        // Windows can retain the SQLite file briefly while an in-flight
        // provider callback releases its connection. Await cleanup rather
        // than treating a transient sharing violation as a room failure.
        let mut cleaned = false;
        for _ in 0..50 {
            if !dir.exists() || std::fs::remove_dir_all(&dir).is_ok() {
                cleaned = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        // The room assertions have already passed. Axum may still have a
        // detached connection task holding a cloned SQLite handle after the
        // listener was aborted. This is fixture teardown, not a room failure.
        if !cleaned {
            eprintln!("room fixture cleanup deferred (open Windows handle): {}", dir.display());
        }
    }
    #[tokio::test]
    async fn stopped_generation_and_uncertain_turns_never_replay() {
        let (state, dir) = test_state();
        let job = save_owner_job(&state, "j", "얘기하자");
        state.cancel_session("room:r");
        let result = run(&run_context(&state, "j"), &job).await.unwrap();
        assert_eq!(result["status"], "cancelled");
        assert_eq!(result["pauseReason"], "job_cancel_requested");
        assert_eq!(state.store.count("execution"), 0);
        let job=state.store.put("job",&json!({"id":"uncertain","status":"running","input":{"roomId":"r"},"roomState":{"sessionId":"room:r","inFlight":true}})).unwrap();
        let result = run(&run_context(&state, "uncertain"), &job).await.unwrap();
        assert_eq!(result["status"], "needs_review");
        assert_eq!(state.store.count("execution"), 0);
        drop(state); std::fs::remove_dir_all(dir).unwrap();
    }
    #[tokio::test]
    async fn fresh_owner_job_after_stop_uses_new_epoch_and_gets_a_real_reply() {
        let (state,dir)=test_state();
        state.store.patch_if("room","r","id",&json!("r"),
            &json!({"agentIds":["a"],"maxTurns":1}));
        let (server,bodies)=provider(&state,vec![text_delta("새 메시지에 대한 답")],None).await;
        let stale=save_owner_job(&state,"room-stale","지난 요청");
        state.cancel_session("room:r");
        let stopped_result=run(&run_context(&state,"room-stale"),&stale).await.unwrap();
        assert_eq!(stopped_result["status"],"cancelled");
        assert_eq!(stopped_result["turns"],0);
        assert_eq!(bodies.lock().unwrap().len(),0);
        let next=save_owner_job(&state,"room-fresh","새 요청");
        let generation=next["input"]["generation"].as_u64().unwrap();
        assert!(generation>0);
        let original_input=next["input"].clone();
        let completed=run(&run_context(&state,"room-fresh"),&next).await.unwrap();
        assert_eq!(completed["status"],"completed","{completed}");
        let saved=state.store.get("job","room-fresh").unwrap();
        assert_eq!(saved["input"],original_input,
            "room checkpoints may not rewrite the owner's frozen request");
        assert_eq!(saved["roomBinding"]["sessionId"],"room:r");
        assert_eq!(saved["roomBinding"]["generation"],generation);
        assert_eq!(completed["turns"],1);
        assert_eq!(completed["outputs"][0]["content"],"새 메시지에 대한 답");
        assert_eq!(completed["outputs"][0]["modelCalls"],1);
        assert!(completed["outputs"][0]["usage"]["totalTokens"].as_u64().unwrap_or(0)>0);
        assert_eq!(bodies.lock().unwrap().len(),1);
        server.abort();
        drop(state); std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn opted_in_owner_command_executes_real_tools_with_frozen_policy_and_evidence() {
        let (state, dir) = test_state();
        state.store.patch_if(
            "room",
            "r",
            "id",
            &json!("r"),
            &json!({"agentIds":["a"],"maxTurns":1,"toolsEnabled":true,"contextBudgetTokens":1}),
        );
        state
            .store
            .put("policy", &json!({"id":"default","autoApproveAll":true}));
        let tool = json!({"tool_calls":[{"index":0,"id":"write-proof","type":"function","function":{"name":"workspace_write","arguments":json!({"path":"owner-proof.txt","content":"executed by Rust"}).to_string()}}]});
        let (server, bodies) = provider(
            &state,
            vec![tool, text_delta("파일을 실제로 저장했어")],
            None,
        )
        .await;
        let job = save_owner_job(
            &state,
            "tools",
            "내 컴퓨터에 owner-proof.txt 파일을 만들어줘",
        );
        // A queued message keeps the permission snapshot granted by its owner;
        // later UI changes cannot silently widen this particular job's policy.
        state.store.patch_if(
            "room",
            "r",
            "id",
            &json!("r"),
            &json!({"toolsEnabled":false}),
        );
        let result = run(&run_context(&state, "tools"), &job).await.unwrap();
        assert_eq!(result["status"], "completed");
        assert_eq!(
            std::fs::read_to_string(dir.join("owner-proof.txt")).unwrap(),
            "executed by Rust"
        );
        assert!(bodies.lock().unwrap()[0]["tools"]
            .as_array()
            .is_some_and(|tools| !tools.is_empty()));
        let operations = state.store.all("execution_operation");
        assert_eq!(state.store.all("execution")[0]["contextPolicy"],"native-work-context");
        assert_eq!(operations.len(), 1);
        assert_eq!(operations[0]["status"], "completed");
        assert_eq!(operations[0]["tool"], "workspace_write");
        assert_eq!(operations[0]["arguments"]["path"], "owner-proof.txt");
        assert_eq!(
            state
                .store
                .messages("room:r")
                .iter()
                .filter(|message| message["role"] == "user")
                .count(),
            1
        );
        server.abort();
        drop(state); std::fs::remove_dir_all(dir).unwrap();
    }
    #[tokio::test]
    async fn completed_checkpoints_and_compatibility_runs_do_not_repeat_owner_or_speakers() {
        let (state, dir) = test_state();
        state.store.patch_if(
            "room",
            "r",
            "id",
            &json!("r"),
            &json!({"agentIds":["a"],"maxTurns":1}),
        );
        let (server, bodies) = provider(&state, vec![text_delta("안녕")], None).await;
        let job=state.store.put("job",&json!({"id":"legacy","status":"running","input":{"roomId":"r","goal":"안녕","generation":0}})).unwrap();
        let context = run_context(&state, "legacy");
        let first = run(&context, &job).await.unwrap();
        assert_eq!(first["turns"], 1);
        let checkpoint = state.store.get("job", "legacy").unwrap();
        let second = run(&context, &checkpoint).await.unwrap();
        assert_eq!(second["turns"], 1);
        assert_eq!(bodies.lock().unwrap().len(), 1);
        assert_eq!(
            state
                .store
                .messages("room:r")
                .iter()
                .filter(|message| message["role"] == "user")
                .count(),
            1
        );
        server.abort();
        drop(context); drop(state); std::fs::remove_dir_all(dir).unwrap();
    }
    #[tokio::test]
    async fn nine_participants_share_one_owner_message_and_all_speak() {
        let (state, dir) = test_state();
        let ids = (1..=9)
            .map(|index| format!("agent-{index}"))
            .collect::<Vec<_>>();
        for id in &ids {
            state.store.put(
                "agent",
                &json!({"id":id,"name":id,"providerId":"p","model":"test"}),
            );
        }
        state.store.patch_if(
            "room",
            "r",
            "id",
            &json!("r"),
            &json!({"agentIds":ids,"maxTurns":9}),
        );
        let deltas = (1..=9)
            .map(|index| text_delta(&format!("{index}번 참가자의 의견")))
            .collect();
        let (server, bodies) = provider(&state, deltas, None).await;
        let job = save_owner_job(&state, "nine", "다 같이 편하게 얘기해보자");
        let result = run(&run_context(&state, "nine"), &job).await.unwrap();
        assert_eq!(result["turns"], 9);
        let outputs=result["outputs"].as_array().unwrap();
        for output in outputs {
            assert_eq!(output["modelCalls"],1);
            assert_eq!(output["toolCallCount"],0);
            assert_eq!(output["usage"]["estimated"],true,
                "mocked providers report no exact token usage");
            assert!(output["usage"]["totalTokens"].as_u64().unwrap_or(0)>0);
            assert!(output["inputContext"]["breakdown"]["components"]
                ["systemAndPersona"].as_u64().unwrap_or(0)>0);
            assert!(output["inputContext"]["breakdown"]["totalEstimatedTokens"]
                .as_u64().unwrap_or(0)>0);
            assert_eq!(output["inputContext"]["breakdown"]["estimated"],true);
        }
        let messages = state.store.messages("room:r");
        assert_eq!(
            messages
                .iter()
                .filter(|message| message["role"] == "user")
                .count(),
            1
        );
        assert_eq!(
            messages
                .iter()
                .filter(|message| message["role"] == "assistant")
                .map(|message| message["agent_id"].as_str().unwrap().to_string())
                .collect::<Vec<_>>(),
            ids
        );
        assert_eq!(bodies.lock().unwrap().len(), 9);
        server.abort();
        drop(state); std::fs::remove_dir_all(dir).unwrap();
    }
    #[tokio::test]
    async fn nine_speakers_complete_three_owner_turns_without_false_completion() {
        let (state,dir)=test_state();
        let ids=(0..9).map(|n|format!("agent-{n}")).collect::<Vec<_>>();
        for id in &ids {
            state.store.put("agent",&json!({"id":id,"name":id,"providerId":"p","model":"test"}));
        }
        state.store.patch_if("room","r","id",&json!("r"),
            &json!({"agentIds":ids,"routingMode":"discussion",
                "rounds":1,"maxTurns":9,"autoLearning":false}));
        let deltas=(0..27).map(|n|text_delta(&format!(
            "speaker reply {n}: committed result for its owner turn"
        ))).collect::<Vec<_>>();
        let (server,bodies)=provider(&state,deltas,None).await;
        for turn in 0..3 {
            let job_id=format!("three-turn-job-{turn}");
            let job=save_owner_job(&state,&job_id,&format!(
                "All nine participants, discuss the next iteration {turn}"
            ));
            let result=run(&run_context(&state,&job_id),&job).await.unwrap();
            assert_eq!(result["status"],"completed","{result}");
            assert_eq!(result["turns"],9,"{result}");
            assert_eq!(result["outputs"].as_array().unwrap().len(),9);
            for output in result["outputs"].as_array().unwrap() {
                assert!(output["executionId"].as_str().is_some());
                assert!(output["usage"]["totalTokens"].as_u64().unwrap_or(0)>0);
                assert_eq!(output["modelCalls"],1);
            }
        }
        let messages=state.store.messages("room:r");
        assert_eq!(messages.len(),30,
            "3 immutable owner turns and 27 saved agent answers");
        assert_eq!(messages.iter().filter(|row|row["role"]=="user").count(),3);
        assert_eq!(messages.iter().filter(|row|row["role"]=="assistant").count(),27);
        assert_eq!(bodies.lock().unwrap().len(),27,
            "every agent answer needs a real provider call");
        server.abort();
        drop(state);std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn discussion_mentions_cannot_starve_the_remaining_first_speakers() {
        let mut pending = VecDeque::from(vec!["c".to_string()]);
        let spoken = HashSet::from(["a".to_string(), "b".to_string()]);
        route_peers(&mut pending, &["a".to_string()], "b", 2, 6, Some(&spoken));
        assert_eq!(pending, VecDeque::from(vec!["c".to_string()]));
        route_peers(&mut pending, &["a".to_string()], "b", 2, 6, None);
        assert_eq!(
            pending,
            VecDeque::from(vec!["a".to_string(), "c".to_string()])
        );
    }
    #[test]
    fn stable_room_session_reads_do_not_publish_record_refresh_events() {
        let (state, dir) = test_state();
        let mut events = state.bus.subscribe();
        let first = ensure_room_session(&state, "r").unwrap();
        let initial = events.try_recv().unwrap();
        assert_eq!(initial["type"], "record");
        assert_eq!(initial["kind"], "room");
        let record = room(&state);
        for _ in 0..10 {
            assert_eq!(ensure_room_session(&state, "r").unwrap()["id"], first["id"]);
        }
        assert!(matches!(
            events.try_recv(),
            Err(tokio::sync::broadcast::error::TryRecvError::Empty)
        ));
        assert_eq!(room(&state), record);
        drop(state); std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn default_room_routing_uses_the_multiagent_roles_algorithm() {
        let (state, dir) = test_state();
        crate::engine::algorithms::seed(&state);
        for mut algorithm in state.store.all("algorithm") {
            if algorithm["kind"].as_str() != Some("multiagent.roles") {
                continue;
            }
            algorithm["status"] = json!(if algorithm["implementation"].as_str()
                == Some("mentioned") { "active" } else { "disabled" });
            state.store.put("algorithm", &algorithm);
        }
        let room = json!({"agentIds":["a","b","c"]});
        assert_eq!(routing_mode(&state, &room).unwrap(), "mentioned");
        let explicit = json!({"agentIds":["a","b","c"],"routingMode":"round_robin"});
        assert_eq!(routing_mode(&state, &explicit).unwrap(), "round_robin");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn opt_in_readonly_agents_run_in_isolated_sessions_and_merge_once(){
        let (state,dir)=test_state();
        state.store.put("room",&json!({
            "id":"r","name":"동시 검토","agentIds":["a","b"],
            "routingMode":"discussion","executionMode":"parallel_readonly",
            "rounds":1,"maxTurns":2,"toolsEnabled":false,"autoLearning":false,
        })).unwrap();
        let (server,bodies)=provider(&state,vec![
            text_delta("독립 의견 1"),text_delta("독립 의견 2"),
            text_delta("최종 의견 1"),text_delta("최종 의견 2"),
        ],None).await;
        let mut job=save_owner_job(&state,"parallel-test","독립적으로 검토해주세요");
        job["input"]["agentIds"]=json!(["a","b"]);
        job["input"]["explicitAgentIds"]=json!(true);
        let job=state.store.put("job",&job).unwrap();
        let output=run(&run_context(&state,"parallel-test"),&job).await.unwrap();
        assert_eq!(output["status"],"completed","{output}");
        assert_eq!(output["turns"],2,"{output}");
        assert_eq!(output["outputs"][0]["agentId"],"a");
        assert_eq!(output["outputs"][1]["agentId"],"b");
        assert!(output["outputs"].as_array().unwrap().iter()
            .all(|o|o["parallelReadOnly"]==true && o["toolCallCount"]==0));
        let public=state.store.messages("room:r");
        assert_eq!(public.iter().filter(|m|m["role"]=="user").count(),1);
        assert_eq!(public.iter().filter(|m|m["role"]=="assistant").count(),2);
        let workers=state.store.all("session").into_iter()
            .filter(|s|s["kind"]=="room_worker").collect::<Vec<_>>();
        assert_eq!(workers.len(),2);
        assert_ne!(workers[0]["id"],workers[1]["id"]);
        assert_eq!(state.store.room_work_items("r","parallel-test:owner",12)
            .unwrap().len(),2);
        assert!(bodies.lock().unwrap().len()>=2);
        assert!(state.store.all("execution_operation").is_empty());
        server.abort();
        drop(state);
        let _=std::fs::remove_dir_all(dir);
    }
}
