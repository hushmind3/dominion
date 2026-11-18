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
            .map(|fact|json!({"id":fact["id"],"kind":fact["kind"],"text":fact["text"],"sourceMessageId":fact["sourceMessageId"],"verifiedLocalOwner":fact["verifiedLocalOwner"]}))
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

/// Exact active owner facts form a monotonic evidence ledger. A later owner
/// fact is safe to append, whereas an edited/superseded old fact must invalidate
/// the prefix, including when the lexical selector did not reselect it.
fn active_owner_facts(record: &Value) -> Vec<Value> {
    record["facts"].as_array().into_iter().flatten()
        .filter(|fact| fact["status"] == "active")
        // Only revision fingerprints are persisted in the participant cache.
        // Exact wording remains in room_state and the transcript, not another
        // 9-way copy of every potentially long owner instruction.
        .map(|fact| json!(stable_id(&[&json!({
            "id":fact["id"],"kind":fact["kind"],"text":fact["text"],
            "sourceMessageId":fact["sourceMessageId"],"verifiedLocalOwner":fact["verifiedLocalOwner"]
        }).to_string()])))
        .collect()
}

fn owner_fact_metadata_digest(record: &Value) -> String {
    let mut metadata = record.clone();
    metadata["facts"] = json!([]);
    evidence_digest("room_state", &metadata)
}

fn owner_fact_prefix_still_valid(dependency: &Value, record: &Value) -> bool {
    if dependency["kind"] != "room_state"
        || dependency["metadataDigest"] != owner_fact_metadata_digest(record)
    { return false; }
    let Some(previous) = dependency["activeFacts"].as_array() else { return false; };
    let current = active_owner_facts(record);
    previous.iter().all(|old| current.contains(old))
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
        let mut dependency=json!({"kind":kind,"id":id,"digest":evidence_digest(kind, &record)});
        if kind == "room_state" {
            dependency["activeFacts"] = json!(active_owner_facts(&record));
            dependency["metadataDigest"] = json!(owner_fact_metadata_digest(&record));
        }
        Some(dependency)
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

/// Immutable room-owner evidence can be accumulated as a semantic delta.
///
/// A different owner question can retrieve a *different subset* of the very
/// same authenticated facts. Treating this as an edit to the old snapshot
/// forces a full provider-cache rebase and also loses previously seen facts.
/// Append only newly selected verbatim fact lines instead. Actual edits,
/// supersession and scope changes are checked by the owner-state dependency
/// revision in project() *before* this optimization runs.
fn append_owner_fact_deltas(fresh: &[Value], prior: &[Value]) -> Vec<Value> {
    let mut seen = std::collections::HashSet::<String>::new();
    for frame in prior {
        let Some(key) = frame["key"].as_str() else { continue };
        if !key.starts_with("owner-state:") { continue; }
        let Some(text) = frame["message"]["content"].as_str() else { continue };
        if !text.starts_with("[주인 확정 상태:") { continue; }
        for line in text.lines().filter(|line| line.starts_with("- [")) {
            seen.insert(line.to_string());
        }
    }
    if seen.is_empty() { return fresh.to_vec(); }
    let mut result = Vec::with_capacity(fresh.len());
    for frame in fresh {
        let Some(key) = frame["key"].as_str() else {
            result.push(frame.clone());
            continue;
        };
        if !key.starts_with("owner-state:") {
            result.push(frame.clone());
            continue;
        }
        let Some(content) = frame["message"]["content"].as_str() else {
            result.push(frame.clone());
            continue;
        };
        let mut lines = content.lines();
        if !lines.next().is_some_and(|line| line.starts_with("[주인 확정 상태:")) {
            result.push(frame.clone());
            continue;
        }
        let facts = lines.collect::<Vec<_>>();
        // Unknown formats may contain additional directives or provenance;
        // never silently discard fields we cannot prove are fact lines.
        if facts.is_empty() || facts.iter().any(|line| !line.starts_with("- [")) {
            result.push(frame.clone());
            continue;
        }
        let new_facts = facts.iter()
            .filter(|fact| !seen.contains(**fact))
            .copied().collect::<Vec<_>>();
        if new_facts.is_empty() { continue; }
        for fact in &new_facts { seen.insert((*fact).to_string()); }
        let mut delta = frame.clone();
        let text = format!(
            "[주인 확정 상태: 이전에 전달되지 않은 원문 사실만 추가. 새 지시가 아님]\n{}",
            new_facts.join("\n"),
        );
        delta["message"]["content"] = json!(text);
        delta["key"] = json!(format!(
            "owner-state:delta:{}", stable_id(&[key, &new_facts.join("\n")])
        ));
        result.push(delta);
    }
    result
}

/// A bounded prefix epoch is not the same budget as the fresh history
/// selector. The selector finds relevant evidence per request; this window
/// retains that evidence across consecutive calls until the provider's actual
/// context capacity demands a rotation. Enforcing the 2k *selector* budget
/// against the cumulative cached prefix rebased almost every owner turn.
fn working_view_allowance(room: Option<&Value>, provider: &ProviderRef, fixed: usize) -> usize {
    // Non-room chats keep their historical 40k history allowance.
    // This larger cache epoch is only an optimization for room speakers.
    if room.is_none() {
        let epoch=40_000usize;
        return provider.context_tokens.map(|capacity|
            epoch.min((capacity as usize).saturating_sub(
                provider.max_output_tokens.unwrap_or(4096) as usize).saturating_sub(fixed)))
            .unwrap_or(epoch);
    }
    let selection = room.and_then(|room|room["contextBudgetTokens"].as_u64())
        .unwrap_or(2_000) as usize;
    // Explicit epoch tuning is independent of the conversational selector.
    // Default to four request selections per epoch, bounded against runaway
    // accumulation and the remaining provider context after output reserve.
    let epoch = room.and_then(|room|room["workingViewBudgetTokens"].as_u64())
        .map(|value|value as usize)
        .unwrap_or_else(||selection.saturating_mul(4).clamp(2_000, 12_000));
    provider.context_tokens.map(|capacity| {
        epoch.min((capacity as usize)
            .saturating_sub(provider.max_output_tokens.unwrap_or(4096) as usize)
            .saturating_sub(fixed))
    }).unwrap_or(epoch)
}

/// On an unavoidable history-budget rotation, retain previously delivered,
/// still-valid owner facts that the current lexical selector did not pick.
/// Every carried line is rechecked against the current active room-state
/// ledger. No old transcript/peer text, revoked decision, or foreign memory
/// is promoted into the new snapshot.
fn retain_verified_owner_facts(
    state: &AppState, session_id: &str, source_id: &str,
    prior: &[Value], fresh: &mut Vec<Value>,
    prior_dependencies: &[Value], token_allowance: usize,
) -> usize {
    let session=state.store.get("session",session_id).unwrap_or(Value::Null);
    let Some(room_id)=session["roomId"].as_str() else{return 0;};
    let expected_id=format!("room-state-{}",stable_id(&[room_id]));
    // Only current-room owner ledger IDs already proven by this participant.
    let has_proof=prior_dependencies.iter().any(|dep|
        dep["kind"]=="room_state" && dep["id"]==expected_id
            && dep["activeFacts"].is_array() && dep["metadataDigest"].is_string());
    if !has_proof {return 0;}
    let Some(record)=state.store.get("room_state",&expected_id) else{return 0;};
    let history=state.store.messages(session_id);
    let Some(cursor)=history.iter().position(|row|row["id"]==source_id)
        else{return 0;};
    let visible=history[..=cursor].iter()
        .filter(|row|row["role"]=="user" && row["metadata"]["authorKind"]=="owner")
        .filter_map(|row|row["id"].as_str().map(str::to_string))
        .collect::<std::collections::HashSet<_>>();
    let mut present=std::collections::HashSet::<String>::new();
    for frame in fresh.iter() {
        if !frame["key"].as_str().is_some_and(|key|key.starts_with("owner-state:")) { continue; }
        if let Some(content)=frame["message"]["content"].as_str() {
            present.extend(content.lines().filter(|line|line.starts_with("- ["))
                .map(str::to_string));
        }
    }
    // Never introduce an owner fact when the compiler did not provide owner
    // state at all; this protects the source-cursor visibility boundary.
    if !fresh.iter().any(|frame|frame["key"].as_str()
        .is_some_and(|key|key.starts_with("owner-state:"))) {return 0;}
    // Proofs were already validated by project(). Resolve only that
    // participant's room-state IDs: scanning all rooms can cross tenants and
    // gets slower as unrelated rooms accumulate.
    let allowed=record["facts"].as_array().into_iter().flatten()
        .filter(|fact| fact["verifiedLocalOwner"]==true
            && fact["sourceMessageId"].as_str().is_some_and(|id|visible.contains(id))
            && (fact["status"]=="active"
                || (fact["status"]=="superseded"
                    && fact["supersededBy"].as_str()
                        .is_some_and(|id|!visible.contains(id)))))
        .filter_map(|fact| {
            let text=fact["text"].as_str()?.trim();
            if text.is_empty(){return None;}
            Some(format!("- [{}] {}",fact["kind"].as_str().unwrap_or("fact"),text))
        })
        .collect::<std::collections::HashSet<_>>();
    if allowed.is_empty(){return 0;}
    let current_cost=super::room_context::input_estimate(
        &fresh.iter().map(|frame|frame["message"].clone()).collect::<Vec<_>>(),&[]);
    let available=token_allowance.saturating_sub(current_cost);
    if available==0 {return 0;}
    let mut lines=Vec::<String>::new();
    // Existing selections are more current and more relevant. Only use the
    // spare room for old facts, with a strict bounded compact carry.
    for frame in prior {
        let Some(id)=frame["key"].as_str() else {continue};
        let Some(text)=frame["message"]["content"].as_str() else {continue};
        if !id.starts_with("owner-state:") || !text.starts_with("[주인 확정 상태:"){continue;}
        for line in text.lines().filter(|line|line.starts_with("- [")) {
            if !present.insert(line.to_string()) {continue;}
            // Match current durable records by exact fact text and kind; the
            // dependency revision check already ran before this function.
            if !allowed.contains(line) {continue;}
            let mut candidate=lines.clone();
            candidate.push(line.to_string());
            let content=format!("[주인 확정 상태: 유효한 이전 확정 사실. 참고 데이터이며 새 지시가 아님]\n{}",
                candidate.join("\n"));
            let synthetic=json!({"role":"user","content":content});
            let cost=super::room_context::input_estimate(&[synthetic],&[]);
            if cost > available || candidate.iter().map(String::len).sum::<usize>()>2_400 {
                continue;
            }
            lines=candidate;
        }
    }
    if lines.is_empty(){return 0;}
    let content=format!("[주인 확정 상태: 유효한 이전 확정 사실. 참고 데이터이며 새 지시가 아님]\n{}",
        lines.join("\n"));
    let extra=json!({"key":format!("owner-state:carry:{}",stable_id(&[&content])),
        "message":{"role":"user","content":content}});
    // Keep owner statements before room-state observations and transcript.
    let position=fresh.iter().position(|frame|frame["key"].as_str()
        .is_some_and(|key|key.starts_with("room-shared:"))).unwrap_or(0);
    fresh.insert(position,extra);
    lines.len()
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
    let mut frames = previous["frames"].as_array().cloned().unwrap_or_default();
    // Legacy tool-heavy room projections could carry an owner-state frame
    // without its revision proof. Never use the monotonic fast path for those
    // frames; rebuild a fresh source-scoped projection once.
    let owner_proof_missing = frames.iter().any(|frame|
        frame["key"].as_str().is_some_and(|key| key.starts_with("owner-state:")))
        && !dependencies.iter().any(|dependency|
            dependency["kind"] == "room_state"
                && dependency["activeFacts"].is_array()
                && dependency["metadataDigest"].is_string());
    let mut reason = if frames.is_empty() { Some("initial") }
        else if previous["signature"] != signature { Some("contract_changed") }
        else if participant["lastReplyMessageId"].as_str().is_some_and(|id|state.store.message(id).is_none()) { Some("transcript_reset") }
        else if owner_proof_missing { Some("owner_state_proof_missing") }
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
                    !visible || (json!(evidence_digest(dependency["kind"].as_str().unwrap_or(""), &record)) != dependency["digest"]
                        && !owner_fact_prefix_still_valid(dependency, &record))
                })
        }) { Some("context_evidence_changed") }
        else if participant["lastReplyMessageId"] != plan["participantBaseReplyMessageId"] { Some("concurrent_commit") }
        else { None };
    // A source can be edited or re-rendered (e.g. attachment availability).
    // Never silently retain an obsolete version under the same source ID.
    // Keep the full fresh snapshot for any genuine rebase. Only an appendable
    // projection may replace repeated owner-state snapshots with fact deltas.
    let appended = if reason.is_none() {
        append_owner_fact_deltas(&fresh, &frames)
    } else {
        fresh.clone()
    };
    if reason.is_none() && appended.iter().any(|new| frames.iter()
        .any(|old| old["key"] == new["key"] && old["message"] != new["message"])) {
        reason = Some("source_changed");
    }
    // Distinguish the room-wide observation from its owner-turn cursor.
    // A different actual response/work status must replace the prior frame:
    // accumulating both would teach the model mutually inconsistent states.
    let current_shared = fresh.iter()
        .filter_map(|frame|frame["key"].as_str())
        .find(|key|key.starts_with("room-shared:"));
    let stale_shared_frames = current_shared.map(|current| frames.iter()
        .filter(|frame|frame["key"].as_str().is_some_and(|key|
            key.starts_with("room-shared:") && key != current)).count())
        .unwrap_or(0);
    if reason.is_none() && stale_shared_frames > 0 {
        reason = Some("room_snapshot_rotated");
    }
    let previous_frames = frames.len();
    let mut reused = frames.len();
    if reason.is_none() {
        let seen = frames.iter().filter_map(|frame|frame["key"].as_str().map(str::to_string))
            .collect::<std::collections::HashSet<_>>();
        frames.extend(appended.iter().filter(|frame|!seen.contains(frame["key"].as_str().unwrap_or(""))).cloned());
        let session = state.store.get("session", session_id).unwrap_or(Value::Null);
        let room = session["roomId"].as_str().and_then(|id|state.store.get("room", id));
        // Independent bounded epoch budget: the 2k per-request selector is
        // not an instruction to evict a useful cacheable prefix every turn.
        let fixed = super::room_context::input_estimate(&[wire_message(&messages[0])], tools);
        let allowance = working_view_allowance(room.as_ref(), provider, fixed);
        let history_cost = |rows:&[Value]| super::room_context::input_estimate(
            &rows.iter().map(|frame|frame["message"].clone()).collect::<Vec<_>>(), &[]);
        if history_cost(&frames) > allowance.max(history_cost(&fresh)) {
            reason = Some("history_budget");
        }
    }
    let mut carried_owner_facts=0;
    let mut epoch = previous["epoch"].as_u64().unwrap_or(0);
    if reason.is_some() {
        let mut rebased=fresh;
        // Other reasons (revocation, edited source, changed provider contract)
        // intentionally discard old frames. Only a budget rotation can carry
        // independently verified immutable owner evidence forward.
        if reason == Some("history_budget") {
            let session=state.store.get("session",session_id).unwrap_or(Value::Null);
            let room=session["roomId"].as_str().and_then(|id|state.store.get("room",id));
            let fixed=super::room_context::input_estimate(&[wire_message(&messages[0])],tools);
            let allowance=working_view_allowance(room.as_ref(),provider,fixed);
            carried_owner_facts=retain_verified_owner_facts(
                state,session_id,source,&frames,&mut rebased,&dependencies,allowance);
        }
        frames=rebased;
        dependencies=fresh_dependencies;
        reused=0;
        epoch+=1;
    }
    else {
        for dependency in fresh_dependencies {
            if let Some(previous) = dependencies.iter_mut().find(|old|
                old["kind"] == dependency["kind"] && old["id"] == dependency["id"]) {
                // Refresh the evidence proof after a safe monotonic addition,
                // so later revocation of the new fact also invalidates it.
                *previous = dependency;
            } else {
                dependencies.push(dependency);
            }
        }
    }
    // Host-side prefix lengths are useful for diagnosing rebases, but are
    // never billed/provider cache hits. Count old frames and appended frames
    // independently; transport and provider usage are separate measurements.
    let estimate_frames = |slice: &[Value]| -> usize {
        super::room_context::estimate(&json!(slice.iter()
            .map(|frame| &frame["message"]).collect::<Vec<_>>()).to_string())
    };
    let reused_estimate = if reused == 0 { 0 } else { estimate_frames(&frames[..reused]) };
    let new_estimate = if reused == frames.len() { 0 } else { estimate_frames(&frames[reused..]) };
    let mut projected = vec![wire_message(&messages[0])];
    projected.extend(frames.iter().map(|frame|frame["message"].clone()));
    plan["workingView"] = json!({
        "epoch":epoch,"reusedFrames":reused,"appendedFrames":frames.len().saturating_sub(reused),
        "rebaseReason":reason,"sourceMessageId":source,
        "previousFrames":previous_frames,"reclaimedStaleRoomSnapshots":stale_shared_frames,
        "carriedVerifiedOwnerFacts":carried_owner_facts,
        "estimatedReusedHistoryTokens":reused_estimate,
        "estimatedNewHistoryTokens":new_estimate,
        "historyEstimateKind":"host-projection-not-provider-cache-or-billing",
        "inputDigest":stable_id(&[&json!(projected).to_string()]),
        "estimatedInputTokens":super::room_context::input_estimate(&projected, tools),
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
    // Cache affinity belongs to the bot, not the room. Transport cursors
    // above remain conversation-local so a cache hint cannot mix histories.
    let identity = super::context::identity_prefix(state, request).0;
    let affinity = stable_id(&[
        "agent-cache-v2", request.agent_id.as_deref().unwrap_or(session_id),
        &provider.id, &provider.base_url, &provider.model,
        provider.account_id.as_deref().unwrap_or(""), &provider.kind,
        &provider.reasoning_protocol, &identity,
    ]);
    let key = format!("dominion_{affinity}");
    // Workers AI routes a stable affinity to the same model instance.
    // Room speakers may reuse the SAME stable room-level prefix while their
    // existing per-agent cache keys and conversation cursors stay isolated.
    let session_affinity = if state.store.get("session",session_id)
        .is_some_and(|session|session["roomId"].is_string()){
        // All speakers in ONE conversation reuse the route; a room reset
        // creates a new session and therefore a fresh cache affinity.
        stable_id(&[
            "cloudflare-room-session-affinity-v1",session_id,&provider.id,
            &provider.base_url,&provider.model,
            provider.account_id.as_deref().unwrap_or(""),
        ])
    }else{affinity.clone()};
    let previous = state.store.get("model_conversation", &id);
    let same_prefix = previous.as_ref().is_some_and(|record|record["prefixDigest"] == prefix);
    let mut record = previous.unwrap_or_else(||json!({
        "id":id,"sessionId":session_id,"agentId":request.agent_id,
        "providerId":provider.id,"model":provider.model,
        "createdAt":chrono::Utc::now().to_rfc3339(),
    }));
    record["transportSessionId"] = json!(scope);
    record["cacheAffinityId"] = json!(affinity);
    record["sessionAffinityId"] = json!(session_affinity);
    record["cacheKey"] = json!(key);
    record["prefixDigest"] = json!(prefix);
    record["lastRequestedAt"] = json!(chrono::Utc::now().to_rfc3339());
    state.store.put("model_conversation", &record)
        .ok_or("봇의 지속 대화 상태를 저장할 수 없습니다.")?;
    Ok(json!({
        "id":id,"participantId":participant_id,"transport":provider.kind,"mode":"prefix-reuse",
        "transportSessionId":scope,"cacheAffinityId":affinity,"sessionAffinityId":session_affinity,
        "cacheKey":key,"prefixDigest":prefix,
        "samePrefixAsPreviousRequest":same_prefix,
        "previousReplyMessageId":participant.get("lastReplyMessageId").unwrap_or(&record["lastReplyMessageId"]),
        "participantBaseReplyMessageId":participant["lastReplyMessageId"],
        "previousRouteReplyMessageId":record["lastReplyMessageId"],
    }))
}

/// Only a durably stored visible answer advances the bot's continuation cursor.
pub(crate) fn commit(state: &AppState, continuity: &Value, message: &Value) {
    let Some(id) = continuity["id"].as_str() else { return };
    let Some(saved) = message["id"].as_str().and_then(|id|state.store.message(id)) else { return };
    let Some(record) = state.store.get("model_conversation", id) else { return };
    if saved["role"] != "assistant" || saved["session_id"] != record["sessionId"]
        || saved["agent_id"] != record["agentId"] { return; }
    if state.store.patch_if("model_conversation", id, "cacheKey", &continuity["cacheKey"], &json!({
        "lastReplyMessageId":saved["id"],
        "lastSourceMessageId":saved["metadata"]["replyToMessageId"],
        "lastCompletedAt":chrono::Utc::now().to_rfc3339(),
        "lastUsage":saved["metadata"]["usage"],
    })).is_none() {return;}
    if let Some(participant_id) = continuity["participantId"].as_str() {
        let _ = state.store.patch_if("participant_conversation", participant_id, "lastReplyMessageId", &continuity["participantBaseReplyMessageId"], &json!({
            "lastReplyMessageId":saved["id"],"lastSourceMessageId":saved["metadata"]["replyToMessageId"],
            "lastCompletedAt":chrono::Utc::now().to_rfc3339(),"lastRouteId":id,
        }));
    }
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

/// Workers AI documents session-affinity as the native cache routing hint.
/// Its OpenAI-compatible endpoint is not guaranteed to accept the unrelated
/// prompt_cache_key JSON field: never burn an HTTP 400 + retry probing it.
fn cloudflare_route(state:&AppState,provider:&ProviderRef)->bool{
    provider.base_url.to_ascii_lowercase().contains("cloudflare.com")
        || state.store.get("provider",&provider.id)
            .is_some_and(|record|record["integration"]=="cloudflare-workers-ai")
}

/// Both HTTP and WebSocket must send the same negotiated cache identity.
pub(super) fn hinted_body(state: &AppState, provider: &ProviderRef, original: &Value, plan: &mut Value, wire: CacheWire) -> Value {
    if matches!(wire,CacheWire::Key) && cloudflare_route(state,provider){
        plan["cacheHint"]=json!("x-session-affinity");
        plan["wireHintSent"]=json!(false);
        plan["cacheHintAcceptance"]=Value::Null;
        return original.clone();
    }
    let known=state.store.get("provider_wire_feature", &feature_id(provider,wire));
    let disabled=known.as_ref().is_some_and(|record|record["accepted"]==false
        && record["expiresAt"].as_i64().is_some_and(|until|until>chrono::Utc::now().timestamp()));
    let mut body=original.clone();
    let hinted=!disabled && apply_hint(&mut body,plan,wire);
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
    let integration=state.store.get("provider",&provider.id)
        .and_then(|record|record["integration"].as_str().map(str::to_string))
        .unwrap_or_default();
    let opencode_go=integration=="opencode-go";
    let cloudflare=cloudflare_route(state,provider);
    for attempt in 1..=2 {
        plan["httpRequests"]=json!(attempt);
        let mut request=builder.try_clone().ok_or("모델 요청을 복제할 수 없습니다.")?;
        if let Some(id)=plan["cacheAffinityId"].as_str().or_else(||plan["transportSessionId"].as_str()) {
            request=request.header("x-dominion-conversation",id);
            // OpenCode Go's direct endpoint uses the same stable conversation
            // affinity under its own header. No local proxy is involved.
            if opencode_go { request=request.header("x-opencode-session",id); }
        }
        if cloudflare {
            if let Some(id)=plan["sessionAffinityId"].as_str()
                .or_else(||plan["cacheAffinityId"].as_str()) {
                // Provider-specific routing hint; NOT an authorization token.
                // A successful HTTP response does not prove a KV cache hit.
                request=request.header("x-session-affinity",id);
                plan["providerSessionAffinitySent"]=json!(true);
            }
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
        assert_eq!(first["cacheKey"],another_room["cacheKey"]);
        assert_eq!(first["cacheAffinityId"],another_room["cacheAffinityId"]);
        assert_ne!(first["participantId"],another_room["participantId"]);
        assert_ne!(first["transportSessionId"],another_room["transportSessionId"]);
        assert!(another_room["previousReplyMessageId"].is_null());
        let private_chat=other(&provider,&request,"private:one","private chat contract");
        assert_eq!(first["cacheKey"],private_chat["cacheKey"]);
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
        crate::test_support::cleanup_dir(root);
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
    async fn cloudflare_wire_affinity_is_room_stable_and_is_not_sent_to_other_providers() {
        use axum::{extract::State,http::HeaderMap,routing::post,Json,Router};
        use std::sync::{Arc,Mutex};
        type Seen=Arc<Mutex<Vec<Option<String>>>>;
        async fn endpoint(State(seen):State<Seen>,headers:HeaderMap,
            Json(_body):Json<Value>)->Json<Value>{
            seen.lock().unwrap().push(headers.get("x-session-affinity")
                .and_then(|value|value.to_str().ok()).map(str::to_string));
            Json(json!({"ok":true}))
        }
        let seen:Seen=Arc::new(Mutex::new(Vec::new()));
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr=listener.local_addr().unwrap();
        let app=Router::new().route("/test",post(endpoint)).with_state(seen.clone());
        let server=tokio::spawn(async move{axum::serve(listener,app).await.unwrap()});
        let root=std::env::temp_dir().join(format!("dominion-cloudflare-affinity-{}",uuid::Uuid::new_v4()));
        let state=state(&root);
        for (id,room) in [("room:one","one"),("room:two","two")]{
            state.store.put("session",&json!({"id":id,"roomId":room})).unwrap();
        }
        let mut provider=route();
        provider.kind="openai".into();
        provider.base_url="https://api.cloudflare.com/client/v4/accounts/acct/ai/v1".into();
        let mut plans=Vec::new();
        for (agent,session) in [("a","room:one"),("b","room:one"),("a","room:two"),("a","private:one")]{
            let request=TurnRequest::from_value(&json!({"agentId":agent,"input":"hello"})).unwrap();
            let mut plan=prepare(&state,&provider,&request,session,"static-room-prefix",&json!({})).unwrap();
            let original=json!({"model":"test","messages":[{"role":"user","content":"hello"}]});
            let hinted=hinted_body(&state,&provider,&original,&mut plan,CacheWire::Key);
            assert_eq!(hinted,original,"Cloudflare must not receive an unsupported cache-key JSON probe");
            assert_eq!(plan["cacheHint"],"x-session-affinity");
            assert_eq!(plan["wireHintSent"],false);
            let res=send(&state,&provider,reqwest::Client::new().post(format!("http://{addr}/test")),
                &json!({"model":"test","messages":[{"role":"user","content":"hello"}]}),
                &mut plan,CacheWire::Key).await.unwrap();
            assert!(res.status().is_success());
            assert_eq!(plan["providerSessionAffinitySent"],true);
            plans.push(plan);
        }
        assert_eq!(plans[0]["sessionAffinityId"],plans[1]["sessionAffinityId"]);
        assert_ne!(plans[0]["cacheKey"],plans[1]["cacheKey"],
            "Distinct speakers must retain separate cache-key/history scopes");
        assert_ne!(plans[0]["sessionAffinityId"],plans[2]["sessionAffinityId"]);
        assert_ne!(plans[0]["sessionAffinityId"],plans[3]["sessionAffinityId"]);
        let mut ordinary=route();ordinary.kind="openai".into();
        let request=TurnRequest::from_value(&json!({"agentId":"a","input":"hello"})).unwrap();
        let mut plan=prepare(&state,&ordinary,&request,"private:one","identity",&json!({})).unwrap();
        send(&state,&ordinary,reqwest::Client::new().post(format!("http://{addr}/test")),
            &json!({"model":"test","messages":[{"role":"user","content":"hello"}]}),
            &mut plan,CacheWire::Key).await.unwrap();
        assert!(plan["providerSessionAffinitySent"].is_null());
        let got=seen.lock().unwrap();
        assert_eq!(got.len(),5);
        assert_eq!(got[0],got[1]);
        assert_ne!(got[0],got[2]);
        assert_ne!(got[0],got[3]);
        assert_eq!(got[4],None,"Don't send Cloudflare-only headers to unrelated providers");
        drop(got);
        server.abort();
        crate::test_support::cleanup_dir(root);
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
        assert_eq!(seen[0],seen[1]);assert_eq!(seen[1],seen[2]);
        assert_ne!(seen[2],seen[3],"another bot must have a separate route affinity");
        drop(seen);server.abort();let _=std::fs::remove_dir_all(root);
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
        drop(seen);server.abort();let _=std::fs::remove_dir_all(root);
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
        assert!(second_plan["workingView"]["estimatedReusedHistoryTokens"]
            .as_u64().is_some_and(|n|n>0));
        assert!(second_plan["workingView"]["estimatedNewHistoryTokens"]
            .as_u64().is_some_and(|n|n>0));
        assert_eq!(second_plan["workingView"]["historyEstimateKind"],
            "host-projection-not-provider-cache-or-billing");
        // A changed persona invalidates the prefix without dropping the fresh
        // selected evidence, and budget rotation never cuts mandatory input.
        let mut edited=second.clone();edited[0]["content"]=json!("edited identity");
        let (rebased,_)=project(&state,&changed,&edited,&[],"room:test",&mut second_plan);
        assert_eq!(second_plan["workingView"]["rebaseReason"],"contract_changed");
        assert_eq!(second_plan["workingView"]["estimatedReusedHistoryTokens"],0);
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
        crate::test_support::cleanup_dir(root);
    }

    #[test]
    fn identical_room_status_reuses_prefix_but_changed_status_rebases_without_erasing_memory() {
        let root=std::env::temp_dir().join(format!("dominion-room-semantic-delta-{}",uuid::Uuid::new_v4()));
        let state=state(&root);
        state.store.put("session",&json!({"id":"room:semantic","roomId":"semantic"})).unwrap();
        state.store.put("room",&json!({"id":"semantic","contextBudgetTokens":12000})).unwrap();
        let request=TurnRequest::from_value(&json!({"input":"continue","agentId":"a"})).unwrap();
        let provider=route();
        let make=|owner:&str,status:&str,self_reply:Option<&str>| {
            let mut frames=vec![
                json!({"role":"system","content":"stable owner instructions and persona",
                    "_dominion_source":owner,"_dominion_dependencies":[]}),
                json!({"role":"user","content":status,
                    "_dominion_frame":format!("room-shared:{}",stable_id(&[status]))}),
                json!({"role":"user","content":"FIXED_OWNER_DECISION_9000",
                    "_dominion_frame":"owner-state:stable-decision"})
            ];
            if let Some(id)=self_reply {
                frames.push(json!({"role":"assistant","content":"previous self conclusion",
                    "_dominion_frame":id}));
            }
            frames.push(json!({"role":"user","content":format!("current owner {owner}"),
                "_dominion_frame":owner}));
            frames
        };
        let first=make("owner-1","all 9 replies received",None);
        let mut one=prepare(&state,&provider,&request,"room:semantic",
            "stable owner instructions and persona",&json!({})).unwrap();
        let (first_wire,pending)=project(&state,&provider,&first,&[],"room:semantic",&mut one);
        let saved=state.store.write_message(&json!({"id":"reply-1",
            "session_id":"room:semantic","agent_id":"a","role":"assistant",
            "content":"previous self conclusion",
            "metadata":{"replyToMessageId":"owner-1"}})).unwrap();
        commit(&state,&one,&saved);
        commit_projection(&state,&one,&saved,pending.unwrap());

        let second=make("owner-2","all 9 replies received",Some("reply-1"));
        let mut two=prepare(&state,&provider,&request,"room:semantic",
            "stable owner instructions and persona",&json!({})).unwrap();
        let (second_wire,pending)=project(&state,&provider,&second,&[],"room:semantic",&mut two);
        assert!(two["workingView"]["rebaseReason"].is_null());
        assert_eq!(two["workingView"]["reclaimedStaleRoomSnapshots"],0);
        assert_eq!(&second_wire[..first_wire.len()],first_wire.as_slice(),
            "unchanged room outcome must preserve the existing wire prefix");
        assert_eq!(second_wire.iter().filter(|row|
            row["content"]=="all 9 replies received").count(),1,
            "do not resend the same status nine times or once per owner cursor");
        assert!(second_wire.iter().any(|row|row["content"]=="FIXED_OWNER_DECISION_9000"));
        assert!(second_wire.iter().any(|row|row["content"]=="current owner owner-2"));
        assert!(two["workingView"]["estimatedReusedHistoryTokens"].as_u64()
            .is_some_and(|tokens|tokens>0));
        assert!(two["workingView"]["estimatedNewHistoryTokens"].as_u64()
            .is_some_and(|tokens|tokens>0));
        let reply_two=state.store.write_message(&json!({"id":"reply-2",
            "session_id":"room:semantic","agent_id":"a","role":"assistant",
            "content":"previous self conclusion",
            "metadata":{"replyToMessageId":"owner-2"}})).unwrap();
        commit(&state,&two,&reply_two);
        commit_projection(&state,&two,&reply_two,pending.unwrap());

        let third=make("owner-3","one speaker now missing",Some("reply-2"));
        let mut three=prepare(&state,&provider,&request,"room:semantic",
            "stable owner instructions and persona",&json!({})).unwrap();
        let (third_wire,_)=project(&state,&provider,&third,&[],"room:semantic",&mut three);
        assert_eq!(three["workingView"]["rebaseReason"],"room_snapshot_rotated");
        assert_eq!(three["workingView"]["reclaimedStaleRoomSnapshots"],1);
        assert_eq!(three["workingView"]["estimatedReusedHistoryTokens"],0);
        let current=json!(third_wire).to_string();
        assert!(!current.contains("all 9 replies received"),
            "stale outcome must not override newer room facts");
        assert!(current.contains("one speaker now missing"));
        assert!(current.contains("FIXED_OWNER_DECISION_9000"));
        assert!(current.contains("previous self conclusion"));
        assert!(current.contains("current owner owner-3"));
        crate::test_support::cleanup_dir(root);
    }

    #[test]
    fn room_snapshot_is_replaced_at_owner_turn_boundary_without_losing_self_or_peer_context() {
        let root=std::env::temp_dir().join(format!("dominion-room-snapshot-{}",uuid::Uuid::new_v4()));
        let state=state(&root);
        state.store.put("session",&json!({"id":"room:ctx","roomId":"ctx"})).unwrap();
        state.store.put("room",&json!({"id":"ctx","contextBudgetTokens":12000})).unwrap();
        let provider=route();
        let request=TurnRequest::from_value(&json!({"input":"Keep discussing","agentId":"agent-0"})).unwrap();
        let mut previous_reply=None::<String>;
        for turn in 0..16 {
            let owner=format!("owner-{turn}");
            let system=json!({"role":"system","content":"stable owner and agent instructions",
                "_dominion_source":owner,"_dominion_dependencies":[]});
            let mut messages=vec![
                system,
                json!({"role":"user","content":format!("ROOM_STATE_VERSION_{turn}"),
                    "_dominion_frame":format!("room-shared:{owner}")}),
                json!({"role":"user","content":format!("OWNER_DECISION_VERSION_{turn}"),
                    "_dominion_frame":format!("owner-state:version-{turn}")}),
            ];
            if let Some(ref id)=previous_reply {
                messages.push(json!({"role":"assistant","content":format!("MY_PREVIOUS_REPLY_{}",turn-1),
                    "_dominion_frame":id}));
            }
            for peer in 0..4 {
                messages.push(json!({"role":"user","content":format!("PEER_{turn}_{peer}"),
                    "_dominion_frame":format!("peer-{turn}-{peer}")}));
            }
            messages.push(json!({"role":"user","content":format!("OWNER_CURRENT_{turn}"),
                "_dominion_frame":owner}));
            let mut plan=prepare(&state,&provider,&request,"room:ctx","stable owner and agent instructions",&json!({})).unwrap();
            let (wire,pending)=project(&state,&provider,&messages,&[],"room:ctx",&mut plan);
            let body=json!(wire).to_string();
            assert!(body.contains(&format!("ROOM_STATE_VERSION_{turn}")),"{turn}: {body}");
            assert!(body.contains(&format!("OWNER_DECISION_VERSION_{turn}")),"{turn}: {body}");
            assert!(body.contains(&format!("OWNER_CURRENT_{turn}")),"{turn}: {body}");
            assert_eq!(body.matches("ROOM_STATE_VERSION_").count(),1,"{turn}: stale shared snapshot leaked");
            assert_eq!(body.matches("OWNER_DECISION_VERSION_").count(),1,"{turn}: stale owner state leaked");
            assert!(wire.len()<=messages.len(),"history unexpectedly grew across turns");
            if turn>0 {
                assert_eq!(plan["workingView"]["rebaseReason"],"room_snapshot_rotated");
                assert_eq!(plan["workingView"]["reclaimedStaleRoomSnapshots"],1);
                assert!(body.contains(&format!("MY_PREVIOUS_REPLY_{}",turn-1)),
                    "latest self reply must survive the snapshot rotation");
                assert!(!body.contains(&format!("ROOM_STATE_VERSION_{}",turn-1)));
            }
            let saved=state.store.write_message(&json!({
                "id":format!("reply-{turn}"),"session_id":"room:ctx","role":"assistant",
                "agent_id":"agent-0","content":format!("MY_PREVIOUS_REPLY_{turn}"),
                "metadata":{"replyToMessageId":owner}
            })).unwrap();
            commit(&state,&plan,&saved);
            commit_projection(&state,&plan,&saved,pending.unwrap());
            previous_reply=saved["id"].as_str().map(str::to_string);
        }
        let record=state.store.get("participant_conversation",
            prepare(&state,&provider,&request,"room:ctx","stable owner and agent instructions",&json!({}))
                .unwrap()["participantId"].as_str().unwrap()).unwrap();
        let frames=record["workingView"]["frames"].as_array().unwrap();
        assert_eq!(frames.iter().filter(|frame|frame["key"].as_str()
            .is_some_and(|key|key.starts_with("room-shared:"))).count(),1);
        assert_eq!(frames.iter().filter(|frame|frame["key"].as_str()
            .is_some_and(|key|key.starts_with("owner-state:"))).count(),1);
        assert!(frames.len()<=12,"16 turns should not grow the durable working view without bound");
        crate::test_support::cleanup_dir(root);
    }

    #[test]
    fn nine_speaker_hundred_turn_projection_keeps_memory_and_reuses_prefixes() {
        let root=std::env::temp_dir().join(format!("dominion-nine-hundred-{}",uuid::Uuid::new_v4()));
        let state=state(&root);
        state.store.put("session",&json!({"id":"room:long","roomId":"long"})).unwrap();
        state.store.put("room",&json!({"id":"long",
            "contextBudgetTokens":1400,"workingViewBudgetTokens":12000})).unwrap();
        let provider=route();
        let mut prior=std::collections::HashMap::<String,String>::new();
        let mut reused_calls=0usize;
        let mut rotated_calls=0usize;
        let mut projected_new=0usize;
        let mut rebuilt_full=0usize;
        for turn in 0..100 {
            let source=format!("owner-{turn}");
            state.store.write_message(&json!({"id":source,"session_id":"room:long",
                "role":"user","content":format!("owner work item {turn}"),
                "metadata":{"authorKind":"owner"}})).unwrap();
            for index in 0..9 {
                let agent=format!("agent-{index}");
                let request=TurnRequest::from_value(&json!({"input":format!("owner work item {turn}"),
                    "agentId":agent})).unwrap();
                let identity=format!("stable owner and group persona {agent}");
                let status=if turn==0 {"no prior group round"}else{"nine responses received"};
                let mut fresh=vec![
                    json!({"role":"system","content":identity,
                        "_dominion_source":source,"_dominion_dependencies":[]}),
                    json!({"role":"user","content":status,
                        "_dominion_frame":format!("room-shared:{}",stable_id(&[status]))}),
                    json!({"role":"user",
                        "content":"[주인 확정 상태: 원문]\n- [decision] KEEP_DEPLOYMENT_9000",
                        "_dominion_frame":"owner-state:stable"}),
                ];
                if let Some(reply_id)=prior.get(&agent) {
                    fresh.push(json!({"role":"assistant",
                        "content":format!("self answer {}",turn-1),
                        "_dominion_frame":reply_id}));
                }
                fresh.push(json!({"role":"user","content":format!("owner work item {turn}"),
                    "_dominion_frame":source}));
                rebuilt_full+=super::room_context::input_estimate(
                    &fresh.iter().map(wire_message).collect::<Vec<_>>(),&[]);
                let mut plan=prepare(&state,&provider,&request,"room:long",&identity,&json!({})).unwrap();
                let (wire,pending)=project(&state,&provider,&fresh,&[],"room:long",&mut plan);
                let reason=&plan["workingView"]["rebaseReason"];
                if reason.is_null(){reused_calls+=1;}
                if reason=="room_snapshot_rotated"{rotated_calls+=1;}
                projected_new+=plan["workingView"]["estimatedNewHistoryTokens"]
                    .as_u64().unwrap_or(0) as usize;
                assert!(wire.iter().any(|row|row["content"]=="[주인 확정 상태: 원문]\n- [decision] KEEP_DEPLOYMENT_9000"));
                assert!(wire.iter().any(|row|row["content"]==format!("owner work item {turn}")));
                assert_eq!(wire.iter().filter(|row|
                    row["content"]=="nine responses received").count(),usize::from(turn>0));
                if turn>0 {
                    assert!(wire.iter().any(|row|row["content"]==format!("self answer {}",turn-1)));
                }
                let id=format!("reply-{turn}-{index}");
                let saved=state.store.write_message(&json!({"id":id,"session_id":"room:long",
                    "role":"assistant","agent_id":agent,
                    "content":format!("self answer {turn}"),
                    "metadata":{"replyToMessageId":source}})).unwrap();
                commit(&state,&plan,&saved);
                commit_projection(&state,&plan,&saved,pending.unwrap());
                prior.insert(agent,id);
            }
        }
        assert_eq!(prior.len(),9);
        assert_eq!(rotated_calls,9,
            "one changed shared status per agent, not once every owner turn");
        assert!(reused_calls>500,
            "working views should keep their provider prefix between bounded epochs: {reused_calls}");
        assert!(projected_new<rebuilt_full,
            "host-estimated new history should be cheaper than cold full reconstruction: {projected_new}/{rebuilt_full}");
        assert_eq!(state.store.messages("room:long").len(),100*10,
            "all owner and nine speaker original records remain durable");
        crate::test_support::cleanup_dir(root);
    }

    #[test]
    fn budget_carry_rejects_foreign_and_future_room_facts() {
        let root=std::env::temp_dir().join(format!("dominion-owner-scope-{}",uuid::Uuid::new_v4()));
        let state=state(&root);
        state.store.put("session",&json!({"id":"room:scope","roomId":"scope"})).unwrap();
        state.store.put("room",&json!({"id":"scope"})).unwrap();
        for id in ["old","now"] {
            state.store.write_message(&json!({"id":id,"session_id":"room:scope",
                "role":"user","content":id,"metadata":{"authorKind":"owner"}})).unwrap();
        }
        let own_id=format!("room-state-{}",stable_id(&["scope"]));
        let foreign_id=format!("room-state-{}",stable_id(&["foreign"]));
        state.store.put("room_state",&json!({"id":foreign_id,"facts":[
            {"id":"b","kind":"decision","status":"active","text":"FOREIGN",
                "verifiedLocalOwner":true,"sourceMessageId":"old"}
        ]})).unwrap();
        let prior=vec![json!({"key":"owner-state:old",
            "message":{"role":"user",
                "content":"[주인 확정 상태: 원문]\n- [decision] IN_SCOPE\n- [decision] FOREIGN\n- [decision] FUTURE"}})];
        let fresh=vec![json!({"key":"owner-state:current",
            "message":{"role":"user","content":"[주인 확정 상태: 원문]\n- [decision] CURRENT"}})];
        let prior_deps=vec![json!({"kind":"room_state","id":own_id,
            "activeFacts":[],"metadataDigest":"proof"})];
        let mut first=fresh.clone();
        assert_eq!(retain_verified_owner_facts(&state,"room:scope","now",
            &prior,&mut first,&prior_deps,5000),0);
        state.store.put("room_state",&json!({"id":own_id,"facts":[
            {"id":"a","kind":"decision","status":"active","text":"IN_SCOPE",
                "verifiedLocalOwner":true,"sourceMessageId":"old"},
            {"id":"later","kind":"decision","status":"active","text":"FUTURE",
                "verifiedLocalOwner":true,"sourceMessageId":"owner-future"}
        ]})).unwrap();
        let mut second=fresh.clone();
        assert_eq!(retain_verified_owner_facts(&state,"room:scope","now",
            &prior,&mut second,&prior_deps,5000),1);
        let rendered=second.to_string();
        assert!(rendered.contains("IN_SCOPE"));
        assert!(!rendered.contains("FOREIGN"));
        assert!(!rendered.contains("FUTURE"));
        crate::test_support::cleanup_dir(root);
    }

    #[test]
    fn owner_fact_selection_reuses_prefix_and_appends_missing_facts_until_real_revision() {
        let root=std::env::temp_dir().join(format!("dominion-owner-delta-{}",uuid::Uuid::new_v4()));
        let state=state(&root);
        state.store.put("session",&json!({"id":"room:r","roomId":"r"}));
        state.store.put("room",&json!({"id":"r","contextBudgetTokens":12000}));
        state.store.put("room_state",&json!({"id":"owner-record","facts":[
            {"id":"fact-a","status":"active","text":"alpha fact","sourceMessageId":"owner1"}
        ]}));
        let request=TurnRequest::from_value(&json!({"input":"remember","agentId":"a"})).unwrap();
        let provider=route();
        let messages=|source:&str, facts:&str| {
            let deps=dependencies(&state,&[json!({"kind":"owner_state","id":"owner-record"})]);
            vec![
                json!({"role":"system","content":"stable owner identity","_dominion_source":source,"_dominion_dependencies":deps}),
                json!({"role":"user","content":format!("[주인 확정 상태: 엔진 원문]\n{facts}"),
                    "_dominion_frame":"owner-state:owner-record"}),
                json!({"role":"user","content":format!("question {source}"),"_dominion_frame":source}),
            ]
        };
        let alpha="- [decision] ALPHA_VERBATIM";
        let beta="- [decision] BETA_VERBATIM";
        let mut first=prepare(&state,&provider,&request,"room:r","stable owner identity",&json!({})).unwrap();
        let (first_wire,pending)=project(&state,&provider,&messages("owner1",alpha),&[],"room:r",&mut first);
        let answer=state.store.write_message(&json!({"session_id":"room:r","agent_id":"a",
            "role":"assistant","content":"I heard alpha","metadata":{"replyToMessageId":"owner1"}})).unwrap();
        commit(&state,&first,&answer);
        commit_projection(&state,&first,&answer,pending.unwrap());
        // A genuinely new confirmed fact must be appendable too, not only a
        // different lexical subset of an unchanged state record.
        let mut augmented=state.store.get("room_state","owner-record").unwrap();
        augmented["facts"].as_array_mut().unwrap().push(json!({
            "id":"fact-b","status":"active","text":"beta fact","sourceMessageId":"owner2"
        }));
        state.store.put("room_state",&augmented);
        let mut second=prepare(&state,&provider,&request,"room:r","stable owner identity",&json!({})).unwrap();
        let (second_wire,pending)=project(&state,&provider,&messages("owner2",beta),&[],"room:r",&mut second);
        assert!(second["workingView"]["rebaseReason"].is_null(),"{second}");
        assert_eq!(&second_wire[..first_wire.len()],first_wire.as_slice());
        let full=serde_json::to_string(&second_wire).unwrap();
        assert_eq!(full.matches("ALPHA_VERBATIM").count(),1);
        assert_eq!(full.matches("BETA_VERBATIM").count(),1);
        assert_eq!(second["workingView"]["appendedFrames"],2);
        let answer=state.store.write_message(&json!({"session_id":"room:r","agent_id":"a",
            "role":"assistant","content":"I heard beta","metadata":{"replyToMessageId":"owner2"}})).unwrap();
        commit(&state,&second,&answer);
        commit_projection(&state,&second,&answer,pending.unwrap());
        let mut third=prepare(&state,&provider,&request,"room:r","stable owner identity",&json!({})).unwrap();
        let (third_wire,_)=project(&state,&provider,&messages("owner3",alpha),&[],"room:r",&mut third);
        assert!(third["workingView"]["rebaseReason"].is_null(),"{third}");
        assert_eq!(third["workingView"]["appendedFrames"],1,
            "already remembered owner facts must not be retransmitted");
        assert_eq!(serde_json::to_string(&third_wire).unwrap().matches("ALPHA_VERBATIM").count(),1);
        // Source evidence changes are a hard invalidation, not an append-only delta.
        let mut edited=state.store.get("room_state","owner-record").unwrap();
        edited["facts"][0]["status"]=json!("superseded");
        edited["facts"].as_array_mut().unwrap().push(json!({
            "id":"fact-c","status":"active","text":"corrected fact","sourceMessageId":"owner4"
        }));
        state.store.put("room_state",&edited);
        let (replaced,_)=project(&state,&provider,&messages("owner4","- [correction] CORRECTED_ALPHA"),
            &[],"room:r",&mut third);
        assert_eq!(third["workingView"]["rebaseReason"],"context_evidence_changed");
        assert!(!serde_json::to_string(&replaced).unwrap().contains("ALPHA_VERBATIM"));
        assert!(serde_json::to_string(&replaced).unwrap().contains("CORRECTED_ALPHA"));
        drop(state);
        std::fs::remove_dir_all(root).unwrap();
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
        assert!(next["workingView"]["rebaseReason"].is_null());
        assert_eq!(wire.iter().filter(|row|row["tool_calls"].is_array()).count(),1);
        assert_eq!(wire.iter().filter(|row|row["role"]=="tool").count(),1);
        state.store.patch_if("skill","skill","enabled",&json!(true),&json!({"enabled":false}));
        let mut fresh=vec![system,json!({"role":"user","content":"next","_dominion_frame":"owner2"})];
        fresh[0]["_dominion_source"]=json!("owner2");fresh[0]["_dominion_dependencies"]=json!([]);
        let (wire,_)=project(&state,&provider,&fresh,&[],"s",&mut next);
        assert_eq!(next["workingView"]["rebaseReason"],"context_evidence_changed");
        assert!(!json!(wire).to_string().contains("old procedure"));
        state.store.clear_messages("s");
        project(&state,&provider,&fresh,&[],"s",&mut next);
        assert_eq!(next["workingView"]["rebaseReason"],"transcript_reset");
        crate::test_support::cleanup_dir(root);
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
        crate::test_support::cleanup_dir(root);
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
        crate::test_support::cleanup_dir(root);
    }
}
