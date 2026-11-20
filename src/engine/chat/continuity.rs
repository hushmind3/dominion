//! Durable transport identity per participant and model route. A matching
//! prefix is a cache request, never evidence that the provider actually hit it.
use super::{ProviderRef, TurnRequest};
use crate::{engine::memory::stable_id, state::AppState};
use serde_json::{json, Value};

pub(crate) struct Projection {
    frames: Vec<Value>,
    dependencies: Vec<Value>,
    signature: String,
    epoch: u64,
}

/// Transport metadata is engine-owned and must never become API parameters.
fn wire_message(message: &Value) -> Value {
    let mut value = message.clone();
    if let Some(object) = value.as_object_mut() {
        object.remove("_dominion_frame");
        object.remove("_dominion_source");
        object.remove("_dominion_dependencies");
    }
    value
}

fn evidence_digest(kind: &str, record: &Value) -> String {
    // Usage counters and last-used timestamps are not semantic revisions.
    let mut meaning = serde_json::Map::new();
    if kind=="room_state" {
        meaning.insert("facts".into(),json!(record["facts"].as_array().into_iter().flatten()
            .filter(|fact|fact["status"]=="active")
            .map(|fact|json!({"id":fact["id"],"kind":fact["kind"],"text":fact["text"],"sourceMessageId":fact["sourceMessageId"]}))
            .collect::<Vec<_>>()));
    }
    for field in ["text","content","instructions","description","name","lesson","avoid","request",
        "principle","trigger","intervention","preconditions","exceptions","status","enabled",
        "scope","partitionKey","shareWithRooms","shareWithChannels","source","type","expiresAt",
        "agentId","sessionId","roomId","revision","verification","confidence","importance",
        "sourceAuthors","sourceRole","sourceMessageIds","correctsMessageIds","evidenceMessageIds","sources","authority","outcome","supportingModels",
        "supportingCases","parameters","applicability"] {
        // Ranking/confidence updates do not edit the memory shown to the
        // model. Treating these as content changes could flush working views
        // even when their remembered text and provenance were identical.
        if matches!(kind, "memory" | "room_knowledge")
            && matches!(field, "confidence" | "importance" | "revision" | "supportingCases") {
            continue;
        }
        if let Some(value) = record.get(field) { meaning.insert(field.into(),value.clone()); }
    }
    stable_id(&[&Value::Object(meaning).to_string()])
}

pub(crate) fn dependencies(state: &AppState, items: &[Value]) -> Vec<Value> {
    items.iter().filter_map(|item| {
        let kind = match item["kind"].as_str()? {
            "memory"=>"memory", "experience"=>"experience", "skill"=>"skill", "improvement"=>"improvement",
            "room_knowledge"=>"room_knowledge", "room_knowledge_global"=>"experience_principle",
            "learned_method"=>"experience_rule", "owner_state"=>"room_state", _=>return None,
        };
        let id=item["id"].as_str()?;
        let record=state.store.get(kind,id)?;
        Some(json!({"kind":kind,"id":id,"digest":evidence_digest(kind, &record)}))
    }).collect()
}

fn frame_key(message: &Value, source: &str, index: usize) -> String {
    if let Some(id) = message["tool_call_id"].as_str() {
        return format!("tool-result:{id}");
    }
    if let Some(calls) = message["tool_calls"].as_array().filter(|calls|!calls.is_empty()) {
        return format!("tool-call:{}", stable_id(&[&json!(calls.iter().map(|call|&call["id"]).collect::<Vec<_>>()).to_string()]));
    }
    message["_dominion_frame"].as_str().map(str::to_string)
        .unwrap_or_else(|| format!("transient-{}", stable_id(&[source, &index.to_string(), &wire_message(message).to_string()])))
}

/// Extend a committed provider view with truly unseen source frames.
/// A later compact reference to a peer reply is redundant if the exact
/// full source frame is already carried in this participant's prefix.
/// A rebase starts from an empty view and therefore keeps that reference.
fn append_unseen_frames(frames:&mut Vec<Value>,fresh:&[Value]){
    let seen=frames.iter().filter_map(|frame|frame["key"].as_str().map(str::to_string))
        .collect::<std::collections::HashSet<_>>();
    frames.extend(fresh.iter().filter(|frame|{
        let key=frame["key"].as_str().unwrap_or("");
        !seen.contains(key) && !key.strip_prefix("room-peer-index:")
            .is_some_and(|source|seen.contains(source))
    }).cloned());
}

/// Enlarged history is beneficial only after this exact provider route
/// has a committed, successful WS response. When Codex fell back to HTTP,
/// its returned transport is "http" and the next turn keeps the room's
/// normal context budget instead of growing full HTTP resend costs.
fn has_confirmed_ws_history(state:&AppState,plan:&Value)->bool{
    plan["previousRouteReplyMessageId"].as_str()
        .and_then(|id|state.store.message(id))
        .is_some_and(|reply|
            reply["metadata"]["continuity"]["incrementalTransport"]["wire"]=="websocket")
}

/// The room's contextBudgetTokens bounds the FRESH selected context. For an
/// actually persistent Codex WebSocket chain, reusing the older frames costs
/// no transcript bytes on the next turn. Rebase only once that carried-over
/// history approaches a separate, still finite physical model allowance.
/// Generic HTTP/Cloudflare/local routes keep the exact selected budget: they
/// resend full prompts, so growing their carried-over history would cost more.
fn continuation_history_allowance(
    provider:&ProviderRef, selected:usize, fixed:usize, websocket_enabled:bool
)->usize{
    let incremental=provider.kind=="codex" && websocket_enabled;
    let preferred=if incremental{
        selected.max(selected.saturating_mul(4).min(12_000))
    }else{selected};
    provider.context_tokens.map(|capacity|{
        preferred.min((capacity as usize)
            .saturating_sub(provider.max_output_tokens.unwrap_or(4096) as usize)
            .saturating_sub(fixed))
    }).unwrap_or(preferred)
}

/// A selected context is a *delta* to a participant's committed working view.
/// Rebuilding a sliding window on every turn changes its first user boundary,
/// defeating prefix reuse even when the persona and cache key are identical.
/// Keep that prefix verbatim until a bounded context epoch must be rebuilt.
pub(crate) fn project(
    state: &AppState, provider: &ProviderRef, messages: &[Value], tools: &[Value],
    session_id: &str, plan: &mut Value,
) -> (Vec<Value>, Option<Projection>) {
    let clean = || messages.iter().map(wire_message).collect();
    // Private chats, Home SDK runs and delegated jobs already get an
    // evidence-selected context from the Rust context compiler. A persistent
    // participant workingView MUST NOT append previously selected, now
    // irrelevant frames back to that view. That defeated both tool-trace
    // compaction and source-addressed retrieval, and could rebuild a 50k+
    // cached prefix after a short owner follow-up.
    //
    // Room turns have a distinct shared-evidence/cursor contract; preserve
    // their existing append-only working view and prefix reuse.
    let is_room=state.store.get("session",session_id)
        .is_some_and(|session|session["roomId"].is_string());
    if !is_room {
        let selected=clean();
        let selected_cost=super::room_context::input_estimate(&selected,tools);
        plan["workingView"]=json!({
            "mode":"selected-private-context-v1",
            "rebaseReason":"owner_selected_context",
            "estimatedInputTokens":selected_cost,
            "freshHistoryEstimatedTokens":super::room_context::input_estimate(
                &selected.iter().skip(1).cloned().collect::<Vec<_>>(),&[]),
            "retainedHistoryEstimatedTokens":0,
            "newHistoryEstimatedTokens":selected_cost,
            "historyAllowanceTokens":selected_cost,
            "reusedFrames":0,
        });
        return (selected,None);
    }
    let Some(source) = messages.first().and_then(|row| row["_dominion_source"].as_str()) else {
        return (clean(), None);
    };
    let Some(participant) = plan["participantId"].as_str()
        .and_then(|id| state.store.get("participant_conversation", id)) else { return (clean(), None) };
    // The source is frozen by the runtime, not inferred from a mutable latest
    // owner message while another room round could already be running.
    // Rotate legacy batch-based views once; otherwise old aggregate evidence
    // and the new record-based frames could coexist after an upgrade.
    let signature = stable_id(&["record-evidence-v2", &wire_message(&messages[0]).to_string(), &json!(tools).to_string()]);
    let previous = &participant["workingView"];
    let scope=crate::engine::memory::MemoryScope::for_session(&state.store, session_id, participant["agentId"].as_str());
    let fresh_dependencies=messages[0]["_dominion_dependencies"].as_array().cloned().unwrap_or_default();
    let mut dependencies=previous["dependencies"].as_array().cloned().unwrap_or_default();
    let fresh = messages.iter().enumerate().skip(1).map(|(index, message)| {
        let body = wire_message(message);
        let key = frame_key(message, source, index);
        json!({"key":key,"message":body})
    }).collect::<Vec<_>>();
    // Compute once before the rebase branch moves 'fresh' into frames.
    let fresh_history_estimated_tokens=super::room_context::input_estimate(
        &fresh.iter().map(|f|f["message"].clone()).collect::<Vec<_>>(),&[]);
    let mut frames = previous["frames"].as_array().cloned().unwrap_or_default();
    let mut reason = if frames.is_empty() { Some("initial") }
        else if previous["signature"] != signature { Some("contract_changed") }
        else if participant["lastReplyMessageId"].as_str().is_some_and(|id|state.store.message(id).is_none()) { Some("transcript_reset") }
        else if dependencies.iter().any(|dependency| {
            state.store.get(dependency["kind"].as_str().unwrap_or(""),dependency["id"].as_str().unwrap_or(""))
                .is_none_or(|record| {
                    let visible=match dependency["kind"].as_str().unwrap_or("") {
                        "memory"=>crate::engine::memory::recallable(&state.store,&record,&scope),
                        "skill"=>crate::engine::memory::visible(&record,&scope),
                        "experience"=>crate::engine::experience::visible(&record,&scope),
                        "experience_rule"=>crate::engine::experience::rule_visible(&record,&scope),
                        _=>true,
                    };
                    !visible || json!(evidence_digest(dependency["kind"].as_str().unwrap_or(""), &record)) != dependency["digest"]
                })
        }) { Some("context_evidence_changed") }
        else if participant["lastReplyMessageId"] != plan["participantBaseReplyMessageId"] { Some("concurrent_commit") }
        else { None };
    // A source can be edited or re-rendered (e.g. attachment availability).
    // Never silently retain an obsolete version under the same source ID.
    if reason.is_none() && fresh.iter().any(|new| frames.iter()
        .any(|old| old["key"] == new["key"] && old["message"] != new["message"])) {
        reason = Some("source_changed");
    }
    // A changed room-wide observation supersedes the old one. The previous
    // response count or work status is historical, not a second current fact.
    // Rotate the snapshot on this boundary, while stable observations reuse
    // their exact prior frame (and remain prefix-cache candidates).
    let current_shared=fresh.iter().filter_map(|frame|frame["key"].as_str())
        .find(|key|key.starts_with("room-shared:"));
    if reason.is_none() && current_shared.is_some_and(|current| frames.iter()
        .any(|frame|frame["key"].as_str().is_some_and(|old|
            old.starts_with("room-shared:") && old!=current))) {
        reason=Some("room_snapshot_rotated");
    }
    let mut reused = frames.len();
    if reason.is_none() {
        append_unseen_frames(&mut frames,&fresh);
        let session = state.store.get("session", session_id).unwrap_or(Value::Null);
        let room = session["roomId"].as_str().and_then(|id|state.store.get("room", id));
        // This is a *history* allowance in addition to mandatory identity, not
        // a cap that discards persona or the current selected request.
        let allowance = room.as_ref().map(|room|room["contextBudgetTokens"].as_u64().unwrap_or(2_000) as usize)
            .unwrap_or(40_000);
        let fixed = super::room_context::input_estimate(&[wire_message(&messages[0])], tools);
        let websocket_enabled=has_confirmed_ws_history(state,plan)
            && !std::env::var("DOMINION_CODEX_WS")
                .is_ok_and(|mode|matches!(mode.as_str(),"off"|"http"));
        let allowance=continuation_history_allowance(provider,allowance,fixed,websocket_enabled);
        plan["workingViewBudgetTokens"]=json!(allowance);
        let history_cost = |rows:&[Value]| super::room_context::input_estimate(
            &rows.iter().map(|frame|frame["message"].clone()).collect::<Vec<_>>(), &[]);
        if history_cost(&frames) > allowance.max(history_cost(&fresh)) {
            reason = Some("history_budget");
        }
    }
    let mut epoch = previous["epoch"].as_u64().unwrap_or(0);
    if reason.is_some() { frames = fresh; dependencies=fresh_dependencies; reused = 0; epoch += 1; }
    else {
        for dependency in fresh_dependencies {
            if !dependencies.iter().any(|old|old["kind"]==dependency["kind"] && old["id"]==dependency["id"]) {
                dependencies.push(dependency);
            }
        }
    }
    let mut projected = vec![wire_message(&messages[0])];
    projected.extend(frames.iter().map(|frame|frame["message"].clone()));
    plan["workingView"] = json!({
        "epoch":epoch,"reusedFrames":reused,"appendedFrames":frames.len().saturating_sub(reused),
        "rebaseReason":reason,"sourceMessageId":source,
        "inputDigest":stable_id(&[&json!(projected).to_string()]),
        "estimatedInputTokens":super::room_context::input_estimate(&projected, tools),
        "freshHistoryEstimatedTokens":fresh_history_estimated_tokens,
        "retainedHistoryEstimatedTokens":super::room_context::input_estimate(
            &frames.iter().map(|f|f["message"].clone()).collect::<Vec<_>>(),&[]),
        "historyAllowanceTokens":plan["workingViewBudgetTokens"],
        // Estimator for the newly appended transcript slice; this is NOT
        // provider-billed uncached prompt tokens or proof of a cache hit.
        "newHistoryEstimatedTokens":super::room_context::input_estimate(
            &frames[reused.min(frames.len())..].iter()
                .map(|f|f["message"].clone()).collect::<Vec<_>>(),&[]),
    });
    (projected, Some(Projection { frames, dependencies, signature, epoch }))
}

pub(crate) fn commit_projection(state: &AppState, plan: &Value, message: &Value, mut projection: Projection) {
    let Some(saved) = message["id"].as_str().and_then(|id|state.store.message(id)) else { return };
    let Some(participant_id) = plan["participantId"].as_str() else { return };
    let Some(record) = state.store.get("participant_conversation", participant_id) else { return };
    if record["lastReplyMessageId"] != saved["id"] { return; }
    let rendered = super::context::render_history(state, saved["session_id"].as_str().unwrap_or(""),
        saved["agent_id"].as_str(), &[saved.clone()], Vec::new());
    projection.frames.extend(rendered.iter().map(|row|json!({"key":saved["id"],"message":wire_message(row)})));
    let _ = state.store.patch_if("participant_conversation", participant_id, "lastReplyMessageId", &saved["id"], &json!({
        "workingView":{"signature":projection.signature,"epoch":projection.epoch,"frames":projection.frames,"dependencies":projection.dependencies},
    }));
}

/// 2026 source-aware cache diagnostics: never persist private persona
/// bytes or tool arguments, only hashes and bounded tool names. The
/// provider cache decides actual hits; these are explainable prefix revisions.
fn prefix_parts(instructions:&str,reasoning:&Value)->Value{
    let tools=reasoning["tools"].as_array().cloned().unwrap_or_default();
    let names=tools.iter().filter_map(|item|
        item["function"]["name"].as_str().or_else(||item["name"].as_str()))
        .map(str::to_string).collect::<Vec<_>>();
    let mut options=reasoning.clone();
    if let Some(map)=options.as_object_mut(){map.remove("tools");}
    json!({
        "system":stable_id(&["system-prefix-v1",instructions]),
        "toolSchemas":stable_id(&["tool-schemas-v1",&json!(tools).to_string()]),
        "toolNames":names,
        "modelOptions":stable_id(&["model-options-v1",&options.to_string()]),
    })
}

fn prefix_change(previous:Option<&Value>,now:&Value)->&'static str{
    let Some(previous)=previous else{return "initial"};
    let old=&previous["cachePrefixParts"];
    if !old.is_object(){return "legacy_unclassified";}
    if old["system"]!=now["system"]{return "system_or_persona_changed";}
    if old["toolNames"]!=now["toolNames"]{return "tool_set_changed";}
    if old["toolSchemas"]!=now["toolSchemas"]{return "tool_schema_changed";}
    if old["modelOptions"]!=now["modelOptions"]{return "model_options_changed";}
    "prefix_stable"
}

pub(crate) fn prepare(
    state: &AppState, provider: &ProviderRef, request: &TurnRequest,
    session_id: &str, instructions: &str, reasoning: &Value,
) -> Result<Value, String> {
    // Conversation ownership never depends on the currently selected model.
    let participant_id = format!("participant-{}", stable_id(&[
        "participant-session-v1", session_id, request.agent_id.as_deref().unwrap_or(""),
    ]));
    let participant = state.store.get("participant_conversation", &participant_id)
        .or_else(||state.store.put_if_absent("participant_conversation", &json!({
            "id":participant_id,"sessionId":session_id,"agentId":request.agent_id,
            "createdAt":chrono::Utc::now().to_rfc3339(),
        }))).ok_or("봇의 공통 대화 상태를 저장할 수 없습니다.")?;
    let scope = stable_id(&[
        "model-conversation-v1", session_id, request.agent_id.as_deref().unwrap_or(""),
        &provider.id, &provider.base_url, &provider.model,
        provider.account_id.as_deref().unwrap_or(""),
        &provider.kind, &provider.reasoning_protocol,
    ]);
    let id = format!("conversation-{scope}");
    let prefix = stable_id(&[instructions, &reasoning.to_string()]);
    // Go/HTTP prompt affinity is PER CONVERSATION, not just per agent.
    // Reusing one id for the same persona in independent rooms can evict
    // warm prefixes or route unrelated sessions to the same worker.
    // Preserve the shared room charter as identical content without merging
    // distinct participant/session cache namespaces.
    let identity = super::context::identity_prefix(state, request).0;
    let affinity = stable_id(&[
        "participant-cache-v3", session_id,
        request.agent_id.as_deref().unwrap_or(""),
        &provider.id, &provider.base_url, &provider.model,
        provider.account_id.as_deref().unwrap_or(""), &provider.kind,
        &provider.reasoning_protocol, &identity,
    ]);
    let key = format!("dominion_{affinity}");
    // Cloudflare Workers AI routes cache tensors by instance. All 9 agents
    // in one room should visit the same model instance even though private
    // conversation cursors and persona-specific cache keys remain separate.
    let workers_affinity=stable_id(&[
        "workers-ai-session-v1",session_id,&provider.id,&provider.base_url,
        &provider.model,
    ]);
    let previous = state.store.get("model_conversation", &id);
    let current_prefix_parts=prefix_parts(instructions,reasoning);
    let prefix_change=prefix_change(previous.as_ref(),&current_prefix_parts);
    let same_prefix = previous.as_ref().is_some_and(|record|record["prefixDigest"] == prefix);
    let mut record = previous.unwrap_or_else(||json!({
        "id":id,"sessionId":session_id,"agentId":request.agent_id,
        "providerId":provider.id,"model":provider.model,
        "createdAt":chrono::Utc::now().to_rfc3339(),
    }));
    record["transportSessionId"] = json!(scope);
    record["cacheAffinityId"] = json!(affinity);
    record["workersAiSessionAffinity"] = json!(workers_affinity);
    record["cacheKey"] = json!(key);
    record["prefixDigest"] = json!(prefix);
    record["cachePrefixParts"]=current_prefix_parts.clone();
    record["lastRequestedAt"] = json!(chrono::Utc::now().to_rfc3339());
    state.store.put("model_conversation", &record)
        .ok_or("봇의 지속 대화 상태를 저장할 수 없습니다.")?;
    Ok(json!({
        "id":id,"participantId":participant_id,"transport":provider.kind,"mode":"prefix-reuse",
        "transportSessionId":scope,"cacheAffinityId":affinity,
        "workersAiSessionAffinity":workers_affinity,
        "cacheKey":key,"prefixDigest":prefix,
        "samePrefixAsPreviousRequest":same_prefix,
        "prefixChange":prefix_change,
        "prefixParts":current_prefix_parts,
        "previousReplyMessageId":participant.get("lastReplyMessageId").unwrap_or(&record["lastReplyMessageId"]),
        "participantBaseReplyMessageId":participant["lastReplyMessageId"],
        "previousRouteReplyMessageId":record["lastReplyMessageId"],
    }))
}

/// Only a durably stored visible answer advances the bot's continuation cursor.
/// Commit both durable continuation cursors in one SQLite transaction.
/// A late reply from a concurrent turn, or a reply rendered with an obsolete
/// prefix, must not advance either cursor. In particular, an old provider
/// route must not claim a reply rejected by the participant's conversation.
pub(crate) fn commit(state: &AppState, continuity: &Value, message: &Value) {
    let Some(id) = continuity["id"].as_str() else { return };
    let Some(participant_id) = continuity["participantId"].as_str() else { return };
    let Some(saved) = message["id"].as_str().and_then(|id| state.store.message(id)) else { return };
    let Some(record) = state.store.get("model_conversation", id) else { return };
    if saved["role"] != "assistant" || saved["session_id"] != record["sessionId"]
        || saved["agent_id"] != record["agentId"] { return; }
    let timestamp = chrono::Utc::now().to_rfc3339();
    let route_guard = json!({
        "lastReplyMessageId":continuity["previousRouteReplyMessageId"],
        "cacheKey":continuity["cacheKey"],
        "prefixDigest":continuity["prefixDigest"],
        "sessionId":saved["session_id"],
        "agentId":saved["agent_id"],
    });
    let participant_guard = json!({
        "lastReplyMessageId":continuity["participantBaseReplyMessageId"],
        "sessionId":saved["session_id"],
        "agentId":saved["agent_id"],
    });
    let route_patch = json!({
        "lastReplyMessageId":saved["id"],
        "lastSourceMessageId":saved["metadata"]["replyToMessageId"],
        "lastCompletedAt":timestamp,
        "lastUsage":saved["metadata"]["usage"],
    });
    let participant_patch = json!({
        "lastReplyMessageId":saved["id"],
        "lastSourceMessageId":saved["metadata"]["replyToMessageId"],
        "lastCompletedAt":timestamp,
        "lastRouteId":id,
    });
    let _ = state.store.patch_pair_if(
        ("model_conversation", id, &route_guard, &route_patch),
        ("participant_conversation", participant_id, &participant_guard, &participant_patch),
    );
}

/// True only for the first-party Cloudflare Workers AI REST/compat route.
/// An arbitrary local API proxy or another Cloudflare product must not receive
/// a provider-specific header or unsupported JSON cache parameter.
fn is_workers_ai(provider:&ProviderRef)->bool{
    let Ok(url)=reqwest::Url::parse(&provider.base_url) else{return false};
    url.scheme()=="https" && url.host_str()==Some("api.cloudflare.com")
        && url.path().starts_with("/client/v4/accounts/")
        && (url.path().ends_with("/ai") || url.path().contains("/ai/"))
}

/// The official OpenCode Go endpoint requires one stable conversation header.
/// An imported provider may omit its integration label. Recognize ONLY the
/// exact HTTPS origin and /zen/go path, never a same-looking proxy.
fn is_opencode_go(state:&AppState,provider:&ProviderRef)->bool{
    if state.store.get("provider",&provider.id)
        .is_some_and(|record|record["integration"]=="opencode-go"){
        return true;
    }
    crate::engine::gateway::opencode_go_base_url(&provider.base_url)
}

#[derive(Clone, Copy, Debug)]
pub(super) enum CacheWire { Key, Ephemeral }

impl CacheWire {
    fn field(self) -> &'static str { match self { Self::Key=>"prompt_cache_key", Self::Ephemeral=>"cache_control" } }
}

fn apply_hint(body: &mut Value, plan: &Value, wire: CacheWire) -> bool {
    match wire {
        CacheWire::Key => { body["prompt_cache_key"]=plan["cacheKey"].clone(); true }
        CacheWire::Ephemeral => {
            if let Some(system)=body["system"].as_str().filter(|text| !text.is_empty()) {
                body["system"]=json!([{"type":"text","text":system,"cache_control":{"type":"ephemeral"}}]);
                true
            } else if let Some(last)=body["system"].as_array_mut().and_then(|parts|parts.last_mut()) {
                last["cache_control"]=json!({"type":"ephemeral"}); true
            } else {false}
        }
    }
}

pub(super) fn rejects_hint(status: reqwest::StatusCode, text: &str, field: &str) -> bool {
    if !matches!(status.as_u16(),400|422) {return false}
    let lower=text.to_ascii_lowercase();
    let parsed=serde_json::from_str::<Value>(text).unwrap_or(Value::Null);
    let parameter=parsed["error"]["param"].as_str().or_else(||parsed["param"].as_str());
    (parameter.is_some_and(|param|param.contains(field)) || lower.contains(field))
        && ["unsupported", "not supported", "unrecognized", "unknown", "not permitted", "not allowed", "extra_forbidden", "extra inputs"].iter().any(|word|lower.contains(word))
}

fn feature_id(provider: &ProviderRef, wire: CacheWire) -> String {
    format!("wire-{}", stable_id(&[
        &provider.id, &provider.base_url, &provider.kind, &provider.model,
        provider.account_id.as_deref().unwrap_or(""), wire.field(),
    ]))
}

pub(super) fn note_hint(state: &AppState, provider: &ProviderRef, wire: CacheWire, accepted: bool, status: u16) {
    state.store.put("provider_wire_feature", &json!({
        "id":feature_id(provider,wire), "field":wire.field(), "accepted":accepted,
        "observedAt":chrono::Utc::now().to_rfc3339(), "httpStatus":status,
        "expiresAt":if accepted {Value::Null} else {json!(chrono::Utc::now().timestamp()+86400)},
    }));
}

/// Both HTTP and WebSocket must send the same negotiated cache identity.
pub(super) fn hinted_body(state: &AppState, provider: &ProviderRef, original: &Value, plan: &mut Value, wire: CacheWire) -> Value {
    let known=state.store.get("provider_wire_feature", &feature_id(provider,wire));
    let disabled=known.as_ref().is_some_and(|record|record["accepted"]==false
        && record["expiresAt"].as_i64().is_some_and(|until|until>chrono::Utc::now().timestamp()));
    let mut body=original.clone();
    let hinted=!disabled && !(is_workers_ai(provider) && matches!(wire,CacheWire::Key))
        && apply_hint(&mut body,plan,wire);
    plan["cacheHint"]=json!(wire.field());
    plan["wireHintSent"]=json!(hinted);
    plan["cacheHintAcceptance"]=if disabled {json!(false)} else {Value::Null};
    body
}

/// A protocol hint is negotiated once from a specific rejection, never from
/// a model name. Failure of an optional hint must not remove conversation state.
pub(super) async fn send(
    state:&AppState, provider:&ProviderRef, builder:reqwest::RequestBuilder,
    original:&Value, plan:&mut Value, wire:CacheWire,
) -> Result<reqwest::Response,String> {
    let mut body=hinted_body(state,provider,original,plan,wire);
    let mut hinted=plan["wireHintSent"]==true;
    let opencode_go=is_opencode_go(state,provider);
    for attempt in 1..=2 {
        plan["httpRequests"]=json!(attempt);
        let mut request=builder.try_clone().ok_or("모델 요청을 복제할 수 없습니다.")?;
        if is_workers_ai(provider){
            if let Some(affinity)=plan["workersAiSessionAffinity"].as_str(){
                request=request.header("x-session-affinity",affinity);
            }
        }
        if let Some(id)=plan["cacheAffinityId"].as_str().or_else(||plan["transportSessionId"].as_str()) {
            request=request.header("x-dominion-conversation",id);
            // OpenCode Go's direct endpoint uses the same stable conversation
            // affinity under its own header. No local proxy is involved.
            if opencode_go { request=request.header("x-opencode-session",id); }
        }
        let response=request.json(&body).send().await.map_err(|e|format!("{} 연결 실패: {e}",provider.name))?;
        let status=response.status();
        if hinted && attempt==1 && matches!(status.as_u16(),400|422) {
            let headers=response.headers().clone();let version=response.version();
            let bytes=response.bytes().await.map_err(|e|e.to_string())?;
            if rejects_hint(status,&String::from_utf8_lossy(&bytes),wire.field()) {
                note_hint(state,provider,wire,false,status.as_u16());
                body=original.clone();hinted=false;
                plan["wireHintSent"]=json!(false);plan["cacheHintAcceptance"]=json!(false);
                continue;
            }
            // Preserve unrelated errors and routing headers for the existing
            // error handling. No retry on authentication or task failures.
            let mut restored=axum::http::Response::new(bytes);
            *restored.status_mut()=status;*restored.headers_mut()=headers;*restored.version_mut()=version;
            return Ok(reqwest::Response::from(restored));
        }
        if hinted && status.is_success() {
            plan["cacheHintAcceptance"]=json!(true);
            note_hint(state,provider,wire,true,status.as_u16());
        }
        return Ok(response)
    }
    unreachable!("one optional compatibility retry")
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state(root: &std::path::Path) -> AppState {
        let config = crate::config::Config { port:0, data_dir:root.into(), web_dir:root.join("web"), workspace:root.into() };
        AppState::new(config.clone(),crate::db::Store::open(&config.db_path()).unwrap(),crate::vault::Vault::open(&config.key_path()))
    }
    fn route() -> ProviderRef {
        ProviderRef { id:"p".into(),name:"test".into(),base_url:"https://example.test/codex".into(),api_key:"never-store-this".into(),model:"model-a".into(),tool_mode:"native".into(),reasoning_protocol:"openai".into(),local:false,context_tokens:None,max_output_tokens:None,requests_per_minute:None,kind:"codex".into(),account_id:Some("account-a".into()) }
    }
    #[test]
    fn a_committed_ws_transport_not_provider_kind_alone_admits_longer_history(){
        let dir=std::env::temp_dir().join(format!("sub2-ws-audit-{}",uuid::Uuid::new_v4()));
        let state=state(&dir);
        let with=|wire:&str|state.store.write_message(&json!({
            "session_id":"room:a","agent_id":"a","role":"assistant",
            "content":format!("transport {wire}"),
            "metadata":{"continuity":{"incrementalTransport":{"wire":wire}}}
        })).unwrap();
        let previous_ws=with("websocket");
        let previous_http=with("http");
        assert!(has_confirmed_ws_history(&state,
            &json!({"previousRouteReplyMessageId":previous_ws["id"]})));
        assert!(!has_confirmed_ws_history(&state,
            &json!({"previousRouteReplyMessageId":previous_http["id"]})));
        assert!(!has_confirmed_ws_history(&state,
            &json!({"previousRouteReplyMessageId":"unknown"})));
        assert!(!has_confirmed_ws_history(&state,&json!({})));
        drop(state);std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn semantically_stable_room_state_reuses_one_frame_and_changed_state_rotates(){
        let root=std::env::temp_dir().join(format!("dominion-room-status-cache-{}",uuid::Uuid::new_v4()));
        let state=state(&root);
        state.store.put("session",&json!({"id":"room:r","roomId":"r"}));
        state.store.put("room",&json!({"id":"r","contextBudgetTokens":8_000}));
        let mut provider=route(); provider.kind="openai".into();
        let request=TurnRequest::from_value(&json!({"input":"repeat","agentId":"agent"})).unwrap();
        let system=|source:&str|json!({"role":"system","content":"same identity","_dominion_source":source});
        let shared=|version:&str|json!({"role":"user","content":format!("confirmed room state {version}"),
            "_dominion_frame":format!("room-shared:{version}")});
        let owner=|source:&str|json!({"role":"user","content":"owner request",
            "_dominion_frame":source});
        let first=vec![system("owner-1"),shared("stable"),owner("owner-1")];
        let mut first_plan=prepare(&state,&provider,&request,"room:r","same identity",&json!({})).unwrap();
        let (_,projection)=project(&state,&provider,&first,&[],"room:r",&mut first_plan);
        let saved=state.store.write_message(&json!({"session_id":"room:r","agent_id":"agent",
            "role":"assistant","content":"I remember the previous turn",
            "metadata":{"replyToMessageId":"owner-1"}})).unwrap();
        commit(&state,&first_plan,&saved);
        commit_projection(&state,&first_plan,&saved,projection.unwrap());
        let second=vec![system("owner-2"),shared("stable"),owner("owner-2")];
        let mut second_plan=prepare(&state,&provider,&request,"room:r","same identity",&json!({})).unwrap();
        let (stable_wire,_)=project(&state,&provider,&second,&[],"room:r",&mut second_plan);
        assert!(second_plan["workingView"]["rebaseReason"].is_null());
        assert_eq!(stable_wire.iter().filter(|m|m["content"]=="confirmed room state stable").count(),1);
        assert!(json!(stable_wire).to_string().contains("I remember the previous turn"));
        let changed=vec![system("owner-2"),shared("changed"),owner("owner-2")];
        let (changed_wire,_)=project(&state,&provider,&changed,&[],"room:r",&mut second_plan);
        assert_eq!(second_plan["workingView"]["rebaseReason"],"room_snapshot_rotated");
        assert_eq!(changed_wire.iter().filter(|m|m["content"]=="confirmed room state changed").count(),1);
        assert!(!json!(changed_wire).to_string().contains("confirmed room state stable"));
        drop(state); std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn indexed_peer_never_duplicates_a_full_reply_in_committed_prefix(){
        // A prior exact reply already belongs to the provider's working
        // view. Its later index must not produce fresh tokens for the same
        // source; after rebase the index is still eligible for delivery.
        let previous=vec![json!({"key":"peer-original","message":{
            "role":"assistant","content":"exact original participant answer"
        }})];
        let fresh=vec![json!({"key":"room-peer-index:peer-original",
            "message":{"role":"user","content":"brief id peer-original"}})];
        let mut carried=previous.clone();
        append_unseen_frames(&mut carried,&fresh);
        assert_eq!(carried.len(),1,"cannot repeat an already available exact source");
        let mut rebased=Vec::new();
        append_unseen_frames(&mut rebased,&fresh);
        assert_eq!(rebased,fresh,"a true rebase restores the source index");
        append_unseen_frames(&mut rebased,&fresh);
        assert_eq!(rebased.len(),1,"same index ID is not appended twice");
    }

    #[test]
    fn only_persistent_ws_enlarges_bounded_history_not_http_replay(){
        let mut provider=route();
        // Offline test validates the model/provider boundary, not the live
        // provider cache hit rate.
        provider.context_tokens=None;
        assert_eq!(continuation_history_allowance(&provider,2000,500,true),8000);
        assert_eq!(continuation_history_allowance(&provider,6000,500,true),12000);
        assert_eq!(continuation_history_allowance(&provider,2000,500,false),2000,
            "explicit WS opt-out keeps default room window");
        provider.kind="openai".into();
        assert_eq!(continuation_history_allowance(&provider,2000,500,true),2000,
            "normal HTTP input is re-sent in full, no expanded history");
        provider.kind="ollama".into();
        assert_eq!(continuation_history_allowance(&provider,2000,500,true),2000,
            "local stateless completion must not inflate context");
        provider.kind="codex".into();
        provider.context_tokens=Some(4500);
        provider.max_output_tokens=Some(1000);
        assert_eq!(continuation_history_allowance(&provider,2000,500,true),3000,
            "native model context and output reservation remain hard bounds");
    }

    #[test]
    fn stale_cross_route_completion_does_not_half_commit_model_cursor() {
        let root = std::env::temp_dir().join(format!("sub2-atomic-cursors-{}", uuid::Uuid::new_v4()));
        let state = state(&root);
        let req = TurnRequest::from_value(&json!({"input":"do work","agentId":"agent-a"})).unwrap();
        let first_route = route();
        let mut second_route = route();
        second_route.model = "model-b".into();
        let first = prepare(&state, &first_route, &req, "room:atomic", "persona", &json!({})).unwrap();
        let second = prepare(&state, &second_route, &req, "room:atomic", "persona", &json!({})).unwrap();
        let first_reply = state.store.write_message(&json!({
            "session_id":"room:atomic","agent_id":"agent-a","role":"assistant","content":"older"
        })).unwrap();
        let second_reply = state.store.write_message(&json!({
            "session_id":"room:atomic","agent_id":"agent-a","role":"assistant","content":"winner"
        })).unwrap();
        commit(&state, &second, &second_reply);
        // First route guard still matches (null). Without a *joint* CAS,
        // its model route cursor advances even though participant CAS fails.
        commit(&state, &first, &first_reply);
        let participant = state.store.get("participant_conversation",
            second["participantId"].as_str().unwrap()).unwrap();
        let older_route = state.store.get("model_conversation",
            first["id"].as_str().unwrap()).unwrap();
        let winning_route = state.store.get("model_conversation",
            second["id"].as_str().unwrap()).unwrap();
        assert_eq!(participant["lastReplyMessageId"],second_reply["id"]);
        assert!(older_route["lastReplyMessageId"].is_null(),"rejected older route remains unchanged");
        assert_eq!(winning_route["lastReplyMessageId"],second_reply["id"]);

        // A subsequent legitimate turn on the previously losing route
        // must be able to advance both cursors using the new participant base.
        let resumed = prepare(&state, &first_route, &req, "room:atomic", "persona", &json!({})).unwrap();
        let resumed_reply = state.store.write_message(&json!({
            "session_id":"room:atomic","agent_id":"agent-a","role":"assistant","content":"resumed"
        })).unwrap();
        commit(&state, &resumed, &resumed_reply);
        assert_eq!(state.store.get("model_conversation",first["id"].as_str().unwrap())
            .unwrap()["lastReplyMessageId"],resumed_reply["id"]);
        assert_eq!(state.store.get("participant_conversation",first["participantId"].as_str().unwrap())
            .unwrap()["lastReplyMessageId"],resumed_reply["id"]);
        drop(state);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn stale_same_route_and_obsolete_prefix_cannot_rewind_cursors() {
        let root = std::env::temp_dir().join(format!("sub2-prefix-cas-{}",uuid::Uuid::new_v4()));
        let state = state(&root);
        let req = TurnRequest::from_value(&json!({"input":"work","agentId":"agent-a"})).unwrap();
        let provider = route();
        let obsolete = prepare(&state,&provider,&req,"room:prefix","old persona",&json!({})).unwrap();
        let current = prepare(&state,&provider,&req,"room:prefix","new persona",&json!({})).unwrap();
        let reply = state.store.write_message(&json!({
            "session_id":"room:prefix","agent_id":"agent-a","role":"assistant","content":"stale"
        })).unwrap();
        commit(&state,&obsolete,&reply);
        assert!(state.store.get("model_conversation",current["id"].as_str().unwrap())
            .unwrap()["lastReplyMessageId"].is_null());
        let accepted = state.store.write_message(&json!({
            "session_id":"room:prefix","agent_id":"agent-a","role":"assistant","content":"accepted"
        })).unwrap();
        commit(&state,&current,&accepted);
        let late = prepare(&state,&provider,&req,"room:prefix","new persona",&json!({})).unwrap();
        let later = prepare(&state,&provider,&req,"room:prefix","new persona",&json!({})).unwrap();
        let winning = state.store.write_message(&json!({
            "session_id":"room:prefix","agent_id":"agent-a","role":"assistant","content":"newer"
        })).unwrap();
        commit(&state,&later,&winning);
        let losing = state.store.write_message(&json!({
            "session_id":"room:prefix","agent_id":"agent-a","role":"assistant","content":"late stale"
        })).unwrap();
        commit(&state,&late,&losing);
        assert_eq!(state.store.get("model_conversation",current["id"].as_str().unwrap())
            .unwrap()["lastReplyMessageId"],winning["id"]);
        assert_eq!(state.store.get("participant_conversation",current["participantId"].as_str().unwrap())
            .unwrap()["lastReplyMessageId"],winning["id"]);
        drop(state);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn same_persona_and_canonical_tools_keep_prefix_diagnostics_stable(){
        let root=std::env::temp_dir().join(format!("dominion-cache-parts-{}",uuid::Uuid::new_v4()));
        let state=state(&root);
        let provider=route();
        let request=TurnRequest::from_value(&json!({"agentId":"a","input":"first"})).unwrap();
        let tool=|name:&str|json!({"type":"function","function":{
            "name":name,"parameters":{"type":"object"}
        }});
        let mk=|tools:Vec<Value>,effort:&str|json!({
            "tools":super::super::tool_projection::canonical_tool_definitions(tools),
            "thinking":true,"effort":effort,"toolMode":"native"
        });
        let a=tool("workspace_read");
        let b=tool("workspace_write");
        let one=prepare(&state,&provider,&request,"room:one","LONG_STATIC_PERSONA",
            &mk(vec![b.clone(),a.clone()],"low")).unwrap();
        assert_eq!(one["prefixChange"],"initial");
        let two=prepare(&state,&provider,&request,"room:one","LONG_STATIC_PERSONA",
            &mk(vec![a.clone(),b.clone()],"low")).unwrap();
        assert_eq!(two["prefixChange"],"prefix_stable");
        assert_eq!(one["prefixDigest"],two["prefixDigest"]);
        assert_eq!(one["cacheKey"],two["cacheKey"]);
        let settings=prepare(&state,&provider,&request,"room:one","LONG_STATIC_PERSONA",
            &mk(vec![a.clone(),b.clone()],"high")).unwrap();
        assert_eq!(settings["prefixChange"],"model_options_changed");
        assert_eq!(settings["cacheKey"],two["cacheKey"],
            "effort changes do not rotate the session routing key");
        let new_tool=prepare(&state,&provider,&request,"room:one","LONG_STATIC_PERSONA",
            &mk(vec![a.clone()],"high")).unwrap();
        assert_eq!(new_tool["prefixChange"],"tool_set_changed");
        let mut modified=a.clone();
        modified["function"]["description"]=json!("revised schema");
        let schema=prepare(&state,&provider,&request,"room:one","LONG_STATIC_PERSONA",
            &mk(vec![modified],"high")).unwrap();
        assert_eq!(schema["prefixChange"],"tool_schema_changed");
        let identity=prepare(&state,&provider,&request,"room:one","REVISED_PERSONA",
            &mk(vec![a],"high")).unwrap();
        assert_eq!(identity["prefixChange"],"system_or_persona_changed");
        assert!(state.store.get("model_conversation",
            one["id"].as_str().unwrap()).unwrap().to_string()
            .contains("cachePrefixParts"));
        drop(state);std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn continuity_survives_restart_and_separates_bots_routes_and_prefix_revisions() {
        let root=std::env::temp_dir().join(format!("dominion-continuity-{}",uuid::Uuid::new_v4()));
        let state=state(&root);
        let request=TurnRequest::from_value(&json!({"input":"first","agentId":"a"})).unwrap();
        let provider=route();
        let first=prepare(&state,&provider,&request,"room:one","full persona",&json!({"effort":"low"})).unwrap();
        assert_eq!(first["samePrefixAsPreviousRequest"],false);
        let mut next=request.clone();next.input="second, different content".into();
        let second=prepare(&state,&provider,&next,"room:one","full persona",&json!({"effort":"low"})).unwrap();
        assert_eq!(first["cacheKey"],second["cacheKey"]);
        assert_eq!(second["samePrefixAsPreviousRequest"],true);
        commit(&state,&first,&json!({"id":"not-persisted"}));
        let id=first["id"].as_str().unwrap();
        assert!(state.store.get("model_conversation",id).unwrap()["lastReplyMessageId"].is_null());
        let message=state.store.write_message(&json!({"session_id":"room:one","agent_id":"a","role":"assistant","content":"my previous answer","metadata":{"usage":{"cachedPromptTokens":0}}})).unwrap();
        commit(&state,&first,&message);
        let other=|provider:&ProviderRef,request:&TurnRequest,session:&str,prefix:&str|prepare(&state,provider,request,session,prefix,&json!({"effort":"low"})).unwrap();
        let mut peer=request.clone();peer.agent_id=Some("b".into());
        assert_ne!(first["cacheKey"],other(&provider,&peer,"room:one","full persona")["cacheKey"]);
        let another_room = other(&provider,&request,"room:two","another room contract");
        assert_ne!(first["cacheKey"],another_room["cacheKey"]);
        assert_ne!(first["cacheAffinityId"],another_room["cacheAffinityId"]);
        assert_ne!(first["participantId"],another_room["participantId"]);
        assert_ne!(first["transportSessionId"],another_room["transportSessionId"]);
        assert!(another_room["previousReplyMessageId"].is_null());
        let private_chat=other(&provider,&request,"private:one","private chat contract");
        assert_ne!(first["cacheKey"],private_chat["cacheKey"]);
        assert!(private_chat["previousReplyMessageId"].is_null());
        for change in ["model","account","endpoint"] {
            let mut changed=provider.clone();
            match change {"model"=>changed.model="model-b".into(),"account"=>changed.account_id=Some("account-b".into()),_=>changed.base_url="https://other.test".into()}
            assert_ne!(first["cacheKey"],other(&changed,&request,"room:one","full persona")["cacheKey"]);
        }
        let changed_contract=other(&provider,&request,"room:one","edited room contract");
        assert_eq!(first["cacheKey"],changed_contract["cacheKey"]);
        assert_eq!(changed_contract["samePrefixAsPreviousRequest"],false);
        state.store.put("agent",&json!({"id":"a","systemPrompt":"edited persona"}));
        assert_ne!(first["cacheKey"],other(&provider,&request,"room:one","full persona")["cacheKey"]);
        state.store.remove("agent","a");
        assert!(!state.store.get("model_conversation",id).unwrap().to_string().contains("never-store-this"));
        for kind in ["openai","anthropic","ollama","llamacpp","lmstudio"] {
            let mut changed=provider.clone();changed.kind=kind.into();changed.model=format!("{kind}-test");
            let transferred=other(&changed,&request,"room:one","full persona");
            assert_eq!(transferred["participantId"],first["participantId"]);
            assert_eq!(transferred["previousReplyMessageId"],message["id"]);
            assert_eq!(transferred["transport"],kind);
        }
        drop(state);
        let restored=super::tests::state(&root);
        let resumed=prepare(&restored,&provider,&request,"room:one","full persona",&json!({"effort":"low"})).unwrap();
        assert_eq!(first["cacheKey"],resumed["cacheKey"]);
        assert_eq!(resumed["previousReplyMessageId"],message["id"]);
        drop(restored); std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn optional_cache_rejection_must_identify_the_parameter_and_preserve_other_errors() {
        let bad=reqwest::StatusCode::BAD_REQUEST;
        assert!(rejects_hint(bad,r#"{"error":{"message":"Unrecognized request argument supplied: prompt_cache_key"}}"#,"prompt_cache_key"));
        assert!(rejects_hint(bad,r#"{"error":{"param":"system.0.cache_control","message":"not supported"}}"#,"cache_control"));
        assert!(!rejects_hint(bad,r#"{"error":{"message":"model not supported"}}"#,"prompt_cache_key"));
        assert!(!rejects_hint(reqwest::StatusCode::UNAUTHORIZED,"prompt_cache_key not supported","prompt_cache_key"));
        assert!(!rejects_hint(reqwest::StatusCode::TOO_MANY_REQUESTS,"prompt_cache_key not supported","prompt_cache_key"));
    }

    #[tokio::test]
    async fn stable_participant_header_survives_cache_hint_negotiation() {
        use axum::{extract::State,http::HeaderMap,routing::post,Json,Router};
        use std::sync::{Arc,Mutex};
        type Seen=Arc<Mutex<Vec<String>>>;
        async fn endpoint(State(seen):State<Seen>,headers:HeaderMap,Json(body):Json<Value>)->(axum::http::StatusCode,Json<Value>){
            seen.lock().unwrap().push(headers["x-dominion-conversation"].to_str().unwrap().to_string());
            if body.get("prompt_cache_key").is_some(){
                (axum::http::StatusCode::BAD_REQUEST,Json(json!({"error":"prompt_cache_key is unsupported"})))
            }else{(axum::http::StatusCode::OK,Json(json!({"ok":true})))}
        }
        let seen:Seen=Arc::new(Mutex::new(Vec::new()));
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address=listener.local_addr().unwrap();
        let app=Router::new().route("/test",post(endpoint)).with_state(seen.clone());
        let server=tokio::spawn(async move{axum::serve(listener,app).await.unwrap()});
        let root=std::env::temp_dir().join(format!("dominion-affinity-{}",uuid::Uuid::new_v4()));
        let state=state(&root);let mut provider=route();provider.base_url=format!("http://{address}");
        provider.kind="openai".into();
        for (agent,session,text) in [("a","room:one","first"),("a","private:two","second"),("b","room:one","second")] {
            let request=TurnRequest::from_value(&json!({"agentId":agent,"input":text})).unwrap();
            let mut plan=prepare(&state,&provider,&request,session,"identity",&json!({})).unwrap();
            let response=send(&state,&provider,reqwest::Client::new().post(format!("http://{address}/test")),
                &json!({"model":"test","messages":[{"role":"user","content":text}]}),&mut plan,CacheWire::Key).await.unwrap();
            assert!(response.status().is_success());
        }
        let seen=seen.lock().unwrap();
        assert_eq!(seen.len(),4,"one negotiation retry, then no further retries");
        assert_eq!(seen[0],seen[1],"same request retries preserve the affinity");
        assert_ne!(seen[1],seen[2],"independent sessions must not share one affinity");
        assert_ne!(seen[2],seen[3],"other bots have a separate affinity");
        drop(seen);server.abort();let _=drop(state); let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn opencode_go_affinity_is_auto_detected_only_for_the_real_go_endpoint(){
        let root=std::env::temp_dir().join(format!("dominion-go-route-{}",uuid::Uuid::new_v4()));
        let state=state(&root);
        let mut provider=route();
        provider.base_url="https://opencode.ai/zen/go/v1".into();
        assert!(is_opencode_go(&state,&provider),
            "imported Go provider may lack the integration metadata");
        provider.base_url="https://opencode.ai/zen/go".into();
        assert!(is_opencode_go(&state,&provider));
        for url in ["https://opencode.ai/inference/go/v1",
            "https://opencode.ai/inference/go/openai/v1",
            "https://opencode.ai/inference/go"]{
            provider.base_url=url.into();
            assert!(is_opencode_go(&state,&provider),"{url}");
        }
        for url in [
            "https://opencode.ai.evil.example/zen/go/v1",
            "http://opencode.ai/zen/go/v1",
            "https://opencode.ai/zen/god/v1",
            "https://opencode.ai/inference/goose/v1",
            "https://opencode.ai.evil.test/inference/go/openai/v1",
            "https://example.test/zen/go/v1",
        ]{
            provider.base_url=url.into();
            assert!(!is_opencode_go(&state,&provider),"{url}");
        }
        drop(state);std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn opencode_go_direct_sends_the_stable_session_header() {
        use axum::{extract::State,http::HeaderMap,routing::post,Json,Router};
        use std::sync::{Arc,Mutex};
        type Seen=Arc<Mutex<Vec<(String,Option<String>)>>>;
        async fn endpoint(State(seen):State<Seen>,headers:HeaderMap,Json(_body):Json<Value>)->Json<Value>{
            seen.lock().unwrap().push((
                headers["x-dominion-conversation"].to_str().unwrap_or("").to_string(),
                headers.get("x-opencode-session").and_then(|value|value.to_str().ok()).map(str::to_string),
            ));
            Json(json!({"ok":true}))
        }
        let seen:Seen=Arc::new(Mutex::new(Vec::new()));
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address=listener.local_addr().unwrap();
        let app=Router::new().route("/test",post(endpoint)).with_state(seen.clone());
        let server=tokio::spawn(async move{axum::serve(listener,app).await.unwrap()});
        let root=std::env::temp_dir().join(format!("dominion-opencode-go-{}",uuid::Uuid::new_v4()));
        let state=state(&root);
        state.store.put("provider",&json!({"id":"p","integration":"opencode-go"})).unwrap();
        let mut provider=route();provider.base_url=format!("http://{address}");provider.kind="openai".into();
        let request=TurnRequest::from_value(&json!({"agentId":"a","input":"hello"})).unwrap();
        let mut plan=prepare(&state,&provider,&request,"room:one","identity",&json!({})).unwrap();
        let response=send(&state,&provider,reqwest::Client::new().post(format!("http://{address}/test")),
            &json!({"model":"test","messages":[{"role":"user","content":"hello"}]}),&mut plan,CacheWire::Key).await.unwrap();
        assert!(response.status().is_success());
        let seen=seen.lock().unwrap();
        let affinity=plan["cacheAffinityId"].as_str().unwrap();
        assert_eq!(seen[0].0,affinity);
        assert_eq!(seen[0].1.as_deref(),Some(affinity),"direct OpenCode Go must receive the session affinity");
        drop(seen);server.abort();let _=drop(state); let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn workers_ai_affinity_is_room_shared_and_cache_hint_uses_header_only() {
        let root=std::env::temp_dir().join(format!("dominion-cf-cache-{}",uuid::Uuid::new_v4()));
        let state=state(&root);
        let mut provider=route();
        provider.base_url="https://api.cloudflare.com/client/v4/accounts/test/ai/v1".into();
        provider.model="@cf/zai-org/glm-4.7-flash".into();
        assert!(is_workers_ai(&provider));
        let mut first=TurnRequest::from_value(&json!({"input":"question","agentId":"a"})).unwrap();
        let a=prepare(&state,&provider,&first,"room:abc","shared owner charter A",&json!({})).unwrap();
        first.agent_id=Some("b".into());
        let b=prepare(&state,&provider,&first,"room:abc","shared owner charter B",&json!({})).unwrap();
        assert_eq!(a["workersAiSessionAffinity"],b["workersAiSessionAffinity"],
            "all room participants should route to one Cloudflare model instance");
        assert_ne!(a["cacheAffinityId"],b["cacheAffinityId"],
            "participant conversation and persona caches stay separated");
        let mut request_plan=a.clone();
        let body=hinted_body(&state,&provider,&json!({"model":provider.model,
            "messages":[{"role":"user","content":"hello"}]}),
            &mut request_plan,CacheWire::Key);
        assert!(body.get("prompt_cache_key").is_none(),
            "Cloudflare Workers AI does not document OpenAI prompt_cache_key");
        assert_eq!(request_plan["wireHintSent"],false);
        for base in [
            "http://127.0.0.1:9999/v1",
            "https://gateway.ai.cloudflare.com/v1/test",
            "https://api.cloudflare.com/not-the-workers-ai-route",
            "https://api.cloudflare.com.evil.test/client/v4/accounts/x/ai/v1"
        ] {
            provider.base_url=base.into();
            assert!(!is_workers_ai(&provider),"{base} must not receive provider-specific header");
        }
        drop(state);std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn committed_working_view_keeps_wire_prefix_when_selection_slides_and_routes_change() {
        let root=std::env::temp_dir().join(format!("dominion-projection-{}",uuid::Uuid::new_v4()));
        let state=state(&root);
        state.store.put("session",&json!({"id":"room:test","roomId":"test"}));
        state.store.put("room",&json!({"id":"test","contextBudgetTokens":2000}));
        let request=TurnRequest::from_value(&json!({"input":"same words","agentId":"a"})).unwrap();
        let provider=route();
        let frame=|id:&str, text:&str|json!({"role":"user","content":text,"_dominion_frame":id});
        let system=|source:&str|json!({"role":"system","content":"full immutable identity","_dominion_source":source});
        let first=vec![system("owner-1"),frame("older","old correction"),frame("owner-1","same words")];
        let mut plan=prepare(&state,&provider,&request,"room:test","full immutable identity",&json!({})).unwrap();
        let (first_wire,pending)=project(&state,&provider,&first,&[],"room:test",&mut plan);
        assert!(!json!(first_wire).to_string().contains("_dominion_"));
        assert!(state.store.get("participant_conversation",plan["participantId"].as_str().unwrap()).unwrap()["workingView"].is_null(),"uncommitted requests cannot change the working view");
        let saved=state.store.write_message(&json!({"session_id":"room:test","agent_id":"a","role":"assistant","content":"I remember","metadata":{"replyToMessageId":"owner-1"}})).unwrap();
        commit(&state,&plan,&saved);
        commit_projection(&state,&plan,&saved,pending.unwrap());
        let mut changed=provider.clone();changed.kind="ollama".into();changed.model="other-model".into();
        let mut second_plan=prepare(&state,&changed,&request,"room:test","full immutable identity",&json!({})).unwrap();
        // The old correction was omitted by the new selector. Identical text
        // from a NEW owner source must still be appended as a new request.
        let mut own_reply=frame(saved["id"].as_str().unwrap(),"I remember");own_reply["role"]=json!("assistant");
        let second=vec![system("owner-2"),own_reply,frame("owner-2","same words")];
        let (second_wire,_)=project(&state,&changed,&second,&[],"room:test",&mut second_plan);
        assert_eq!(&second_wire[..first_wire.len()],first_wire.as_slice());
        assert_eq!(second_wire.iter().filter(|row|row["content"]=="same words").count(),2);
        assert_eq!(second_plan["workingView"]["reusedFrames"],3);
        assert_eq!(second_plan["workingView"]["appendedFrames"],1);
        // A changed persona invalidates the prefix without dropping the fresh
        // selected evidence, and budget rotation never cuts mandatory input.
        let mut edited=second.clone();edited[0]["content"]=json!("edited identity");
        let (rebased,_)=project(&state,&changed,&edited,&[],"room:test",&mut second_plan);
        assert_eq!(second_plan["workingView"]["rebaseReason"],"contract_changed");
        assert_eq!(rebased.len(),edited.len());
        state.store.put("room",&json!({"id":"test","contextBudgetTokens":1}));
        let (bounded,_)=project(&state,&changed,&second,&[],"room:test",&mut second_plan);
        assert_eq!(second_plan["workingView"]["rebaseReason"],"history_budget");
        assert_eq!(bounded.len(),second.len());
        drop(state);
        let restored=super::tests::state(&root);
        let mut resumed=prepare(&restored,&provider,&request,"room:test","full immutable identity",&json!({})).unwrap();
        restored.store.put("room",&json!({"id":"test","contextBudgetTokens":2000}));
        let (after_restart,_)=project(&restored,&provider,&second,&[],"room:test",&mut resumed);
        assert_eq!(after_restart,second_wire);
        drop(restored); std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn replay_does_not_duplicate_tool_pairs_or_retain_revoked_knowledge() {
        let root=std::env::temp_dir().join(format!("dominion-projection-tools-{}",uuid::Uuid::new_v4()));
        let state=state(&root);
        state.store.put("session",&json!({"id":"s"}));
        state.store.put("skill",&json!({"id":"skill","instructions":"old procedure","enabled":true}));
        let request=TurnRequest::from_value(&json!({"input":"work","agentId":"a"})).unwrap();
        let provider=route();
        let deps=dependencies(&state,&[json!({"kind":"skill","id":"skill"})]);
        let system=json!({"role":"system","content":"identity","_dominion_source":"owner1","_dominion_dependencies":deps});
        let call=json!({"role":"assistant","content":"","tool_calls":[{"id":"call1","type":"function","function":{"name":"read","arguments":"{}"}}]});
        let result=json!({"role":"tool","tool_call_id":"call1","content":"done"});
        let messages=vec![system.clone(),json!({"role":"user","content":"old procedure","_dominion_frame":"owner1"}),call.clone(),result.clone()];
        let mut plan=prepare(&state,&provider,&request,"s","identity",&json!({})).unwrap();
        let (_,pending)=project(&state,&provider,&messages,&[],"s",&mut plan);
        let saved=state.store.write_message(&json!({"session_id":"s","agent_id":"a","role":"assistant","content":"finished","metadata":{"replyToMessageId":"owner1"}})).unwrap();
        commit(&state,&plan,&saved);commit_projection(&state,&plan,&saved,pending.unwrap());
        state.store.patch_if("skill","skill","enabled",&json!(true),&json!({"lastUsedAt":"new telemetry","useCount":100}));
        let mut replay=vec![system.clone(),call,result,json!({"role":"user","content":"next","_dominion_frame":"owner2"})];
        replay[0]["_dominion_source"]=json!("owner2");
        replay[1]["_dominion_frame"]=json!("stored-call-row");replay[2]["_dominion_frame"]=json!("stored-result-row");
        let mut next=prepare(&state,&provider,&request,"s","identity",&json!({})).unwrap();
        let (wire,_)=project(&state,&provider,&replay,&[],"s",&mut next);
        assert_eq!(next["workingView"]["rebaseReason"],"owner_selected_context");
        assert_eq!(wire.iter().filter(|row|row["tool_calls"].is_array()).count(),1);
        assert_eq!(wire.iter().filter(|row|row["role"]=="tool").count(),1);
        state.store.patch_if("skill","skill","enabled",&json!(true),&json!({"enabled":false}));
        let mut fresh=vec![system,json!({"role":"user","content":"next","_dominion_frame":"owner2"})];
        fresh[0]["_dominion_source"]=json!("owner2");fresh[0]["_dominion_dependencies"]=json!([]);
        let (wire,_)=project(&state,&provider,&fresh,&[],"s",&mut next);
        assert_eq!(next["workingView"]["rebaseReason"],"owner_selected_context");
        assert!(!json!(wire).to_string().contains("old procedure"));
        state.store.clear_messages("s");
        project(&state,&provider,&fresh,&[],"s",&mut next);
        assert_eq!(next["workingView"]["rebaseReason"],"owner_selected_context");
        drop(state); std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn private_followups_never_resurrect_prior_expensive_context(){
        let root=std::env::temp_dir().join(format!(
            "dominion-private-view-{}",uuid::Uuid::new_v4()));
        let state=state(&root);
        state.store.put("session",&json!({"id":"private:history"}));
        let provider=route();
        let request=TurnRequest::from_value(&json!({
            "input":"why do I use so many tokens","agentId":"a",
        })).unwrap();
        let system=json!({"role":"system","content":"fixed owner policy",
            "_dominion_source":"message-0"});
        // Emulate a previously committed cache view full of source material.
        let initial=vec![system.clone(),
            json!({"role":"user","content":"search these documents",
                "_dominion_frame":"question-one"}),
            json!({"role":"assistant","content":"OLD_HTML_BODY_".repeat(7000),
                "_dominion_frame":"past-observation"})];
        let mut first=prepare(&state,&provider,&request,"private:history",
            "fixed owner policy",&json!({})).unwrap();
        let (one,pending)=project(&state,&provider,&initial,&[],
            "private:history",&mut first);
        assert!(pending.is_none(),"private turns cannot persist a stale wire view");
        assert!(one.iter().any(|row|row["content"].as_str()
            .is_some_and(|text|text.contains("OLD_HTML_BODY"))));
        let participant_id=first["participantId"].as_str().unwrap();
        state.store.patch_if("participant_conversation",participant_id,"id",
            &json!(participant_id),&json!({"workingView":{
                "frames":[{"key":"old","message":{"role":"assistant",
                    "content":"OLD_HTML_BODY_".repeat(7000)}}]}}));
        for number in 0..6{
            let fresh=vec![system.clone(),json!({"role":"user",
                "_dominion_frame":format!("owner-{number}"),
                "content":format!("말 짧게 해라 {number}")})];
            let mut plan=prepare(&state,&provider,&request,
                "private:history","fixed owner policy",&json!({})).unwrap();
            let (selected,pending)=project(&state,&provider,&fresh,&[],
                "private:history",&mut plan);
            assert!(pending.is_none());
            assert_eq!(selected.len(),2,"one question must stay one question");
            assert!(!json!(selected).to_string().contains("OLD_HTML_BODY"));
            assert_eq!(plan["workingView"]["mode"],"selected-private-context-v1");
            assert!(plan["workingView"]["estimatedInputTokens"].as_u64()
                .unwrap()<400,"selected context must not inherit a huge cache prefix");
        }
        drop(state);
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn retrieval_batches_and_ranking_updates_do_not_resend_identical_evidence() {
        let root=std::env::temp_dir().join(format!("dominion-evidence-delta-{}",uuid::Uuid::new_v4()));
        let state=state(&root);
        state.store.put("session",&json!({"id":"room:r","roomId":"r"}));
        state.store.put("room",&json!({"id":"r","contextBudgetTokens":8000}));
        for id in ["a","b"] {
            state.store.put("memory",&json!({"id":id,"text":format!("FACT_{id}"),"scope":"shared","shareWithRooms":true,"status":"active","importance":0.8,"confidence":0.6}));
        }
        let request=TurnRequest::from_value(&json!({"input":"question","agentId":"agent"})).unwrap();
        let provider=route();
        let rows=|ids:&[&str]|ids.iter().map(|id|json!({"kind":"memory","id":id,"text":format!("FACT_{id}"),"sourceMessageIds":null})).collect::<Vec<_>>();
        let messages=|source:&str,ids:&[&str]|{
            let deps=dependencies(&state,&rows(ids));
            let mut messages=vec![json!({"role":"system","content":"FULL_PERSONA","_dominion_source":source,"_dominion_dependencies":deps})];
            messages.extend(super::super::room_context::evidence_frames(&rows(ids)));
            messages.push(json!({"role":"user","content":format!("question {source}"),"_dominion_frame":source}));
            messages
        };
        let mut first=prepare(&state,&provider,&request,"room:r","FULL_PERSONA",&json!({})).unwrap();
        let (wire,pending)=project(&state,&provider,&messages("owner1",&["a"]),&[],"room:r",&mut first);
        let reply=state.store.write_message(&json!({"session_id":"room:r","agent_id":"agent","role":"assistant","content":"chosen answer"})).unwrap();
        commit(&state,&first,&reply);commit_projection(&state,&first,&reply,pending.unwrap());
        state.store.patch_if("memory","a","status",&json!("active"),&json!({"importance":0.99,"confidence":0.9,"revision":"ranking-update","supportingCases":["another observation"]}));
        let mut next=prepare(&state,&provider,&request,"room:r","FULL_PERSONA",&json!({})).unwrap();
        let (next_wire,_)=project(&state,&provider,&messages("owner2",&["a","b"]),&[],"room:r",&mut next);
        assert!(next["workingView"]["rebaseReason"].is_null(),"{next}");
        assert_eq!(&next_wire[..wire.len()],wire.as_slice());
        let text=json!(next_wire).to_string();
        assert_eq!(text.matches("FACT_a").count(),1);
        assert_eq!(text.matches("FACT_b").count(),1);
        assert!(!text.contains("sourceMessageIds"),"null fields are not evidence");
        // Changes to actual provenance and revocation still invalidate old data.
        state.store.patch_if("memory","a","status",&json!("active"),&json!({"sourceAuthors":["a different author"]}));
        project(&state,&provider,&messages("owner2",&["a","b"]),&[],"room:r",&mut next);
        assert_eq!(next["workingView"]["rebaseReason"],"context_evidence_changed");
        state.store.patch_if("memory","a","status",&json!("active"),&json!({"status":"archived"}));
        let (revoked,_)=project(&state,&provider,&messages("owner2",&["b"]),&[],"room:r",&mut next);
        assert!(!json!(revoked).to_string().contains("FACT_a"));
        drop(state); std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejected_websocket_hint_is_also_disabled_for_http_and_expires() {
        let root=std::env::temp_dir().join(format!("dominion-hint-{}",uuid::Uuid::new_v4()));
        let state=state(&root);let provider=route();
        let mut plan=json!({"cacheKey":"stable-participant-key"});
        let original=json!({"input":[{"role":"user","content":"hello"}]});
        let body=hinted_body(&state,&provider,&original,&mut plan,CacheWire::Key);
        assert_eq!(body["prompt_cache_key"],"stable-participant-key");
        note_hint(&state,&provider,CacheWire::Key,false,400);
        let body=hinted_body(&state,&provider,&original,&mut plan,CacheWire::Key);
        assert_eq!(body,original);assert_eq!(plan["cacheHintAcceptance"],false);
        let id=feature_id(&provider,CacheWire::Key);
        state.store.patch_if("provider_wire_feature",&id,"accepted",&json!(false),&json!({"expiresAt":0}));
        assert_eq!(hinted_body(&state,&provider,&original,&mut plan,CacheWire::Key)["prompt_cache_key"],"stable-participant-key");
        drop(state); std::fs::remove_dir_all(root).unwrap();
    }
}
