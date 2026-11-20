//! Tool effect dispatch, deterministic replacements and durable observations.
use super::*;

/// A fully specified replacement is executable without a model only when every
/// acceptance requirement is the same deterministic operation and its expected
/// hash was frozen before the turn. Anything less falls back to the model.
pub(super) fn frozen_replacements(checks: &Value) -> Option<Vec<Value>> {
    let items = checks.as_array()?;
    (!items.is_empty()
        && items.len() <= 32
        && items.iter().all(|check| {
            check["type"] == "file_replace_exact"
                && check["path"].as_str().is_some_and(|path| !path.is_empty())
                && check["old"].as_str().is_some_and(|old| !old.is_empty())
                && check["new"].as_str().is_some()
                && check["expectedSha256"].as_str().is_some()
        }))
    .then(|| items.to_vec())
}

/// Applies the literal replacement only when the old text occurs once and the
/// result matches the hash captured in the contract. A mismatch is not a
/// failure of the task; it means the deterministic path does not apply.
fn replacement_content(check: &Value, content: &str) -> Option<String> {
    let old = check["old"].as_str()?;
    let new = check["new"].as_str()?;
    if content.matches(old).count() != 1 {
        return None;
    }
    let replaced = content.replacen(old, new, 1);
    (format!("{:x}", Sha256::digest(replaced.as_bytes())) == check["expectedSha256"].as_str()?)
        .then_some(replaced)
}

/// Runs one deterministic tool call through the same approval, file lock,
/// effect receipt and evidence store as a model-issued call.
#[allow(clippy::too_many_arguments)]
async fn deterministic_tool(
    state: &AppState,
    request: &TurnRequest,
    tx: &UnboundedSender<Value>,
    session_id: &str,
    execution_id: &str,
    tool: &str,
    arguments: &Value,
    op_seq: u64,
    metrics: &std::sync::Mutex<Metrics>,
) -> Result<(Value, Value), String> {
    let session=state.store.get("session",session_id).unwrap_or_else(||json!({}));
    let call=ToolCall{id:format!("frozen-{op_seq}-{tool}"),name:tool.into(),arguments:arguments.to_string()};
    let (result,_,_)=dispatch_tool_call(state,tx,&session,request,session_id,execution_id,&call,arguments.clone(),metrics,op_seq,0,op_seq as usize,false,"deterministic_shortcut",None,None,None).await?;
    let operation=state.store.get("execution_operation",&format!("{execution_id}:0:{op_seq}:{}",call.id)).ok_or("결정적 실행 기록이 없습니다.")?;
    Ok((result,operation))
}

/// An explicit owner-directed public web search is a required observable
/// action, not a suggestion to the language model. Execute it exactly once
/// through the same approval/effect/receipt gateway as other tools before
/// the first model token. The tool exchange is provider-compatible and its
/// full receipt remains in SQLite. Failure is surfaced, never turned into
/// an ungrounded answer that pretends a search happened.
#[allow(clippy::too_many_arguments)]
pub(super) async fn run_required_web_search(
    state: &AppState,
    request: &TurnRequest,
    tx: &UnboundedSender<Value>,
    session_id: &str,
    execution_id: &str,
    op_seq: &mut u64,
    metrics: &std::sync::Mutex<Metrics>,
) -> Result<Vec<Value>, String> {
    let Some(query)=super::super::web_research::explicit_query(&request.input) else {
        return Ok(Vec::new());
    };
    if !request.tools_enabled || !request.tools_execution_enabled {
        return Err("웹 검색을 요청하셨지만 현재 채팅의 도구 실행이 꺼져 있습니다. 검색을 수행하지 않았습니다.".into());
    }
    let session=state.store.get("session",session_id).unwrap_or(Value::Null);
    if !tools::profile_tool_visible(&session,"web_search")
        || !tools::profile_tool_allowed(&session,"web_search",&json!({"query":query}))
    {
        return Err("이 세션에서는 웹 검색 도구를 사용할 권한이 없습니다. 검색을 수행하지 않았습니다.".into());
    }
    let next=state.store.execution_operations(execution_id)?
        .iter().filter_map(|op|op["opSeq"].as_u64()).max().unwrap_or(*op_seq);
    *op_seq=(*op_seq).max(next).saturating_add(1);
    let call_id=format!("frozen-{}-web_search",*op_seq);
    let arguments=json!({"query":query});
    let tool_call=json!({"id":call_id,"type":"function",
        "function":{"name":"web_search","arguments":arguments.to_string()}});
    let assistant=json!({"role":"assistant","content":"",
        "tool_calls":[tool_call]});
    let stored=state.store.write_message(&json!({
        "session_id":session_id,"role":"assistant","content":"",
        "agent_id":request.agent_id,"tool_calls":assistant["tool_calls"],
        "metadata":{"authorKind":"harness","origin":"explicit_web_search",
            "executionId":execution_id}
    })).ok_or("웹 검색 호출 기록을 저장하지 못했습니다.")?;
    state.bus.message(&stored);
    let (result,operation)=deterministic_tool(
        state,request,tx,session_id,execution_id,
        "web_search",&arguments,*op_seq,metrics,
    ).await?;
    let count=result["results"].as_array().map(Vec::len).unwrap_or(0);
    if operation["status"]!="completed" || count==0 {
        state.store.audit("chat.web_research_failed",json!({
            "sessionId":session_id,"executionId":execution_id,
            "operationId":operation["id"],"result":result,
        }));
        return Err("실제 웹 검색 결과를 확보하지 못했습니다. 검색했다고 주장하는 답변은 생성하지 않습니다. 연결 상태와 검색 공급자를 확인하세요.".into());
    }
    state.store.audit("chat.web_research_observed",json!({
        "sessionId":session_id,"executionId":execution_id,
        "operationId":operation["id"],"resultCount":count,
    }));
    let _=tx.send(json!({"type":"web_research","status":"verified",
        "results":count,"operationId":operation["id"]}));
    Ok(vec![assistant,json!({"role":"tool",
        "tool_call_id":call_id,"content":result.to_string()})])
}

/// Records what was already applied when a later write cannot proceed. The
/// operations remain the primary evidence; this marker makes the partial state
/// explicit on the execution record.
fn record_partial_shortcut(
    state: &AppState,
    execution_id: &str,
    applied: &[Value],
    failed: &std::path::Path,
    reason: &str,
) {
    if let Some(mut execution) = state.store.get("execution", execution_id) {
        execution["deterministicShortcut"] = json!({
            "kind": "file_replace_exact",
            "modelCalls": 0,
            "tokens": 0,
            "partial": true,
            "appliedEffectIds": applied,
            "failedPath": failed.to_string_lossy(),
            "reason": reason,
            "note": "deterministic execution optimization; excluded from learning evidence",
        });
        state.store.put("execution", &execution);
    }
}

/// Executes every frozen replacement before any model call. Reads are all
/// completed and validated first; a single mismatch falls back to the normal
/// model path without mutating anything.
#[allow(clippy::too_many_arguments)]
pub(super) async fn run_frozen_replacements(
    state: &AppState,
    request: &TurnRequest,
    tx: &UnboundedSender<Value>,
    session_id: &str,
    execution_id: &str,
    task_id: &str,
    text: &str,
    checks: Vec<Value>,
    metrics: &std::sync::Mutex<Metrics>,
) -> Result<Option<Value>, String> {
    if !request.tools_enabled
        || !request.tools_execution_enabled
        || std::env::var("DOMINION_DETERMINISTIC_REPLACEMENT").as_deref() == Ok("0")
    {
        return Ok(None);
    }
    let existing = state.store.execution_operations(execution_id)?;
    let mut op_seq = existing
        .iter()
        .filter_map(|operation| operation["opSeq"].as_u64())
        .max()
        .unwrap_or(0);
    let mut paths = Vec::new();
    for check in &checks {
        let raw = check["path"].as_str().ok_or("경로가 없습니다.")?;
        let path = crate::engine::verification::resolve_path(state, raw)?;
        if paths.iter().any(|existing| existing == &path) {
            // Two independently frozen hashes on one file cannot both be
            // validated after the first change. Let the model handle it.
            return Ok(None);
        }
        paths.push(path);
    }
    let mut prepared = Vec::new();
    for (check, path) in checks.iter().zip(paths) {
        op_seq += 1;
        let (read, _) = deterministic_tool(
            state,
            request,
            tx,
            session_id,
            execution_id,
            "file_read",
            &json!({"path": path.to_string_lossy()}),
            op_seq,
            metrics,
        )
        .await?;
        let Some(content) = read["content"].as_str() else {
            return Ok(None);
        };
        let source_sha256 = format!("{:x}", Sha256::digest(content.as_bytes()));
        let Some(expected) = replacement_content(check, content) else {
            return Ok(None);
        };
        prepared.push((path, source_sha256, expected));
    }
    let mut effect_ids = Vec::new();
    for (path, source_sha256, expected) in prepared {
        op_seq += 1;
        let (_, operation) = deterministic_tool(
            state,
            request,
            tx,
            session_id,
            execution_id,
            "file_write",
            &json!({
                "path": path.to_string_lossy(),
                "content": expected,
                // Compare immediately before writing, under the per-file lock.
                "expectedSha256": source_sha256,
            }),
            op_seq,
            metrics,
        )
        .await?;
        if operation["result"]["staleVersion"].as_bool() == Some(true) {
            record_partial_shortcut(
                state,
                execution_id,
                &effect_ids,
                &path,
                "source version changed before write",
            );
            return Err(format!(
                "{} 파일이 읽은 뒤 변경되어 쓰기를 중단했습니다. 이미 적용된 변경은 실행 기록에 남아 있습니다.",
                path.display()
            ));
        }
        if operation["effectState"] != "verified" {
            record_partial_shortcut(
                state,
                execution_id,
                &effect_ids,
                &path,
                "write was not independently verified",
            );
            return Err(format!(
                "{} 변경을 독립적으로 검증하지 못했습니다. 이미 적용된 변경은 실행 기록에 남아 있습니다.",
                path.display()
            ));
        }
        effect_ids.push(operation["effectId"].clone());
    }
    let answer = "요청한 정확 치환을 모델 호출 없이 수행하고 SHA-256으로 검증했습니다.".to_string();
    if let Some(mut execution) = state.store.get("execution", execution_id) {
        execution["acceptancePassed"] = json!(true);
        execution["finalAnswer"] = json!(answer);
        execution["toolsUsed"] = json!(["file_read", "file_write"]);
        execution["deterministicShortcut"] = json!({
            "kind": "file_replace_exact",
            "modelCalls": 0,
            "tokens": 0,
            "checks": checks.len(),
            "effectIds": effect_ids,
            "note": "deterministic execution optimization; excluded from learning evidence",
        });
        state.store.put("execution", &execution);
    }
    let saved = state
        .store
        .write_message(&json!({
            "session_id": session_id,
            "role": "assistant",
            "content": answer,
            "agent_id": request.agent_id,
            "metadata": {
                "authorKind": "agent",
                "executionId": execution_id,
                "taskId": task_id,
                "deterministicShortcut": true,
                "acceptanceText": text,
            },
        }))
        .ok_or("결정적 실행의 최종 응답을 저장할 수 없습니다.")?;
    if let Some(mut execution) = state.store.get("execution", execution_id) {
        execution["finalMessageId"] = saved["id"].clone();
        state.store.put("execution", &execution);
    }
    state.bus.message(&saved);
    Ok(Some(saved))
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn dispatch_tool_call(
    state: &AppState,
    tx: &UnboundedSender<Value>,
    session: &Value,
    request: &TurnRequest,
    session_id: &str,
    execution_id: &str,
    call: &ToolCall,
    arguments: Value,
    metrics: &std::sync::Mutex<Metrics>,
    op_seq: u64,
    round: usize,
    call_index: usize,
    _compat: bool,
    origin: &str,
    procedure_node: Option<&str>,
    procedure_strategy: Option<&str>,
    procedure_revision: Option<&str>,
) -> Result<(Value, bool, String), String> {
    // Unwrap only tools discovered in this execution. The real tool name
    // proceeds through the same effect ledger, owner approval and sandbox gates.
    // An invalid discovery call becomes an ordinary tool failure, not a bypass
    // or a fatal turn error.
    let bridge = if call.name == crate::engine::capability::CALL {
        Some(crate::engine::capability::resolve_for_agent(
            state, session_id, request.agent_id.as_deref(),
            execution_id, &arguments,
        ).await)
    } else {
        None
    };
    let mut effective_call = None;
    let mut bridge_error = None;
    let arguments = match bridge {
        Some(Ok((name, resolved_args))) => {
            effective_call = Some(ToolCall {
                id: call.id.clone(), name, arguments: resolved_args.to_string(),
            });
            resolved_args
        }
        Some(Err(error)) => {
            bridge_error = Some(error);
            arguments
        }
        None => arguments,
    };
    let call = effective_call.as_ref().unwrap_or(call);
    let argument_strategy = crate::addons::runtime_hooks::choose_for_agent(
        state,
        "tool.arguments",
        request.agent_id.as_deref(),
        Some(execution_id),
    )
    .and_then(|algorithm| {
        algorithm["implementation"]
            .as_str()
            .or_else(|| algorithm["variant"].as_str())
            .map(str::to_string)
    })
    .unwrap_or_default();
    let arguments = normalize_tool_arguments(arguments, &argument_strategy);
    crate::addons::runtime_hooks::mark_signal(state, execution_id, "tool.arguments", true);
    // Programs/adaptations enter the same sub4 journal as model calls. The
    // model dispatcher has already recorded its whole read batch. Product
    // program branches receive child frames linked to this execution.
    let frame_id=format!("{execution_id}:{origin}:{round}:{call_index}:{}",call.id);
    let native_journal=ChatToolJournal{state,execution_id,frame_id:&frame_id,session_id,generation:state.cancellation_generation(session_id)};
    let native_call=dominion_core::domain::ToolCall{id:format!("{origin}:{round}:{call_index}:{}",call.id),name:call.name.clone(),arguments:arguments.clone()};
    let mut native_frame=if origin!="model"{
        if state.store.get("native_frame",&frame_id).is_some(){return Err("이미 기록된 실행 프레임입니다. 결과를 먼저 확인하세요.".into());}
        let mut frame=dominion_core::hosted::Snapshot::new(&frame_id,&bounded_contract_intent(&request.input),1)?;
        frame.native.parent_run_id=uuid::Uuid::parse_str(execution_id).ok();
        dominion_core::journal::validate_new_calls(&frame.native,std::slice::from_ref(&native_call))?;
        frame.native.queued_calls=vec![native_call.clone()];
        frame.native.messages.push(json!({"role":"assistant","content":"","tool_calls":[{"id":native_call.id,"type":"function","function":{"name":native_call.name,"arguments":native_call.arguments.to_string()}}]}));
        dominion_core::journal::begin(&mut frame.native,&native_call)?;
        dominion_core::hosted::Journal::save(&native_journal,&frame)?;Some(frame)
    }else{None};

    // Profile admission happens before approval or mutation-history queries.
    // A child may discover a tool but cannot use that to expand its role.
    let role_admitted = tools::profile_tool_allowed(session, &call.name, &arguments)
        && crate::engine::extensions::tool_allowed(
            state,session_id,request.agent_id.as_deref(),&call.name);
    // Only admitted mutating calls need the session history scan.
    let mut effect_operations = if role_admitted
        && crate::engine::effect::is_effectful(&call.name, &arguments) {
        state.store.session_execution_operations(session_id)?
    } else {
        Vec::new()
    };
    effect_operations.reverse();
    let effect = crate::engine::effect::prepare(
        session_id,
        &call.name,
        &arguments,
        &effect_operations,
    );
    let effect_block = crate::engine::effect::unresolved(&effect, &effect_operations);
    let started_at = chrono::Utc::now().to_rfc3339();
    let tool_started = std::time::Instant::now();
    let _ = tx.send(json!({ "type": "tool_start", "name": call.name, "id": call.id }));

    let needs_approval = if call.name.starts_with("mcp__") {
        crate::engine::extensions::tools::mcp_requires_approval(state, &call.name)
    } else {
        tools::requires_approval(&call.name)
    };
    let operation_id = format!("{execution_id}:{round}:{call_index}:{}", call.id);
    let mut allowed = if !role_admitted || effect_block.is_some() {
        false
    } else if needs_approval {
        request_approval(
            state,
            tx,
            session_id,
            &call.name,
            &arguments,
            execution_id,
            &operation_id,
            procedure_node,
        )
        .await
    } else {
        true
    };

    if let Some(control)=state.store.get("execution",execution_id).and_then(|e|e.get("ownerControl").cloned()){
        if let (Some(kind),Some(id),Some(revision))=(control["kind"].as_str(),control["id"].as_str(),control["revision"].as_u64()){
            if state.store.get(kind,id).is_none_or(|row|row["steeringRevision"].as_u64().unwrap_or(0)!=revision){allowed=false;}
        }
    }
    let initial_effect_state = if effect_block.is_some() {
        "needs_review"
    } else if allowed {
        "started"
    } else {
        "denied"
    };
    state
        .store
        .put(
            "execution_operation",
            &json!({
                "id": operation_id, "runId": execution_id, "sessionId": session_id,
                "callId": call.id, "tool": call.name, "arguments": arguments,
                "opSeq": op_seq, "origin": origin,
                "agentId": request.agent_id.as_deref().or_else(||session["agentId"].as_str()),
                "procedureNodeId": procedure_node,
                "procedureStrategyId": procedure_strategy,
                "procedureRevision": procedure_revision,
                "effectId": effect["effectId"],
                "resourceKey": effect["resourceKey"],
                "requestDigest": effect["requestDigest"],
                "beforeVersion": effect["beforeVersion"],
                "effectState": initial_effect_state,
                "toolReceipt": {
                    "effectId": effect["effectId"],
                    "tool": call.name,
                    "requestDigest": effect["requestDigest"],
                    "resourceKey": effect["resourceKey"],
                    "status": initial_effect_state,
                    "verified": false,
                    "verifier": {"kind": "pending", "revision": "effect-v1"},
                    "beforeVersion": effect["beforeVersion"],
                    "startedAt": started_at,
                },
                "status": "running", "startedAt": started_at,
            }),
        )
        .ok_or("도구 실행 시작을 저장할 수 없습니다.")?;
    let mut file_observation = None;
    let (result, ok) = if !role_admitted {
        let error="위임 역할에 허용되지 않는 도구 또는 명령입니다.";
        let _=tx.send(json!({"type":"tool_error","name":call.name,
            "id":call.id,"error":error}));
        (json!({"error":error,"delegationProfileDenied":true}),false)
    } else if let Some(prior) = &effect_block {
        // The previous attempt has no confirmed outcome. Sending the same
        // non-idempotent change again would risk a duplicate external effect.
        let prior_state = prior["effectState"].as_str().unwrap_or("unknown");
        let error = format!(
            "이전 시도의 효과 상태가 확인되지 않았습니다({prior_state}). 같은 변경을 다시 보내지 말고 대상의 현재 상태를 먼저 조회한 뒤 판단하세요."
        );
        let value = json!({
            "error": error,
            "effectUnknown": true,
            "requiresInspection": true,
            "effectId": effect["effectId"],
            "priorEffectState": prior_state,
            "priorOperationId": prior["id"],
        });
        let _ = tx.send(json!({
            "type": "tool_error",
            "name": call.name,
            "id": call.id,
            "error": "확인되지 않은 이전 효과가 있어 재실행을 보류했습니다.",
            "result": value,
        }));
        (value, false)
    } else if allowed {
        let context = ToolContext {
            workspace: &state.config.workspace,
            store: &state.store,
            state,
            session_id,
            agent_id: request.agent_id.as_deref(),
            tools_enabled: request.tools_enabled,
            tools_execution_enabled: request.tools_execution_enabled,
        };
        let outcome = if let Some(error) = bridge_error.as_ref() {
            Err(error.clone())
        } else if session["delegationWorkspace"].is_object()&&(call.name.starts_with("mcp__")||matches!(call.name.as_str(),"github_api"|"github_file_read"|"github_file_update"|"browser_control")){
            Err("격리 위임 작업에서는 외부 변경 도구를 실행할 수 없습니다.".into())
        } else if !tools::profile_tool_allowed(&session,&call.name,&arguments) {
            Err("위임 역할에 허용되지 않는 도구 또는 명령입니다.".into())
        } else if session["readOnly"]==true&&!tools::observational(&call.name){
            Err("읽기 전용 위임 작업에는 이 도구를 실행할 권한이 없습니다.".into())
        } else if call.name.starts_with("mcp__") {
            crate::engine::extensions::tools::invoke_mcp(
                state,session_id,request.agent_id.as_deref(),&call.name,&arguments).await
        } else {
            tools::execute(&context, &call.name, &arguments, execution_id).await
        };
        match outcome {
            Ok(value) => {
                let mut value = value;
                if matches!(call.name.as_str(), "workspace_read" | "workspace_read_range" | "file_read" | "workspace_replace" | "workspace_replace_range" | "workspace_write") {
                    file_observation = value.as_object_mut()
                        .and_then(|value| value.remove("fileObservation"));
                }
                if tool_succeeded(&value) {
                    let _ = tx.send(json!({
                        "type": "tool_end",
                        "name": call.name,
                        "id": call.id,
                        "arguments": arguments,
                        "result": value,
                    }));
                    (value, true)
                } else {
                    let _ = tx.send(json!({ "type": "tool_error", "name": call.name,
                        "id": call.id, "error": "도구가 실패 결과를 반환했습니다.", "result": value }));
                    (value, false)
                }
            }
            Err(error) if call.name.starts_with("workspace_") && error == tools::WORKSPACE_ESCAPE => {
                let error = format!("{error}. Use the relative path inside the workspace, for example bench/report.txt.");
                let _ = tx.send(json!({"type":"tool_error","name":call.name,"id":call.id,"error":error}));
                (json!({"error":error,"invalidWorkspacePath":true}), false)
            }
            Err(error) => {
                let _ = tx.send(json!({
                    "type": "tool_error",
                    "name": call.name,
                    "id": call.id,
                    "error": error,
                }));
                (json!({ "error": error }), false)
            }
        }
    } else {
        let _ = tx.send(json!({
            "type": "tool_error",
            "name": call.name,
            "id": call.id,
            "error": "사용자가 실행을 거부했습니다.",
        }));
        (json!({ "error": "Denied by the user" }), false)
    };

    {
        let mut counters = metrics.lock().unwrap();
        counters.tool_calls += 1;
        counters.tool_elapsed_ms += tool_started.elapsed().as_millis() as u64;
    }
    record_tool_stat(state, session_id, request.agent_id.as_deref(), &call.name, ok);
    state.store.audit(
        "tool.call",
        json!({
            "tool": call.name,
            "args": arguments,
            "allowed": allowed,
            "round": round,
        }),
    );
    let sealed = crate::engine::effect::seal(
        &state.config.workspace,
        &call.name,
        &arguments,
        &result,
        ok,
        !allowed && effect_block.is_none(),
        effect_block.is_some(),
        &effect,
    )
    .await;
    let operation = state
        .store
        .put(
            "execution_operation",
            &json!({
                "id": operation_id,
                "runId": execution_id,
                "sessionId": session_id,
                "callId": call.id,
                "tool": call.name,
                "arguments": arguments,
                "result": result,
                "fileObservation": file_observation,
                "opSeq": op_seq,
                "origin": origin,
                "agentId": request.agent_id.as_deref().or_else(||session["agentId"].as_str()),
                "procedureNodeId": procedure_node,
                "procedureStrategyId": procedure_strategy,
                "procedureRevision": procedure_revision,
                "effectId": effect["effectId"],
                "resourceKey": effect["resourceKey"],
                "requestDigest": effect["requestDigest"],
                "beforeVersion": effect["beforeVersion"],
                "effectState": sealed["effectState"],
                "observedVersion": sealed["observedVersion"],
                "toolReceipt": sealed["toolReceipt"],
                "status": if ok { "completed" } else { "failed" },
                "startedAt": started_at,
                "finishedAt": chrono::Utc::now().to_rfc3339(),
            }),
        )
        .ok_or("도구 수행 결과를 저장하지 못했습니다. 실행 기록을 확인하세요.")?;
    state.bus.record("execution_operation", &operation);

    // A verified observation of the same resource closes a pending effect for
    // retry purposes. It does not upgrade the old effect to verified.
    if !crate::engine::effect::is_effectful(&call.name, &arguments)
        && sealed["toolReceipt"]["verified"].as_bool() == Some(true)
    {
        if let Some(resource) = effect["resourceKey"].as_str() {
            let operations = state.store.resource_execution_operations(resource)?;
            for updated in crate::engine::effect::mark_inspected(
                &operations,
                resource,
                operation["id"].as_str().unwrap_or(""),
            ) {
                if let Some(saved) = state.store.put("execution_operation", &updated) {
                    state.bus.record("execution_operation", &saved);
                }
            }
        }
    }

    // Keep the full result. The runtime selects a model-specific context
    // window when assembling the next provider request; local and cloud
    // models are treated according to declared capacity, not arbitrary modes.
    let tool_content = result.to_string();
    let saved = state
        .store
        .write_message(&json!({
            "session_id": session_id,
            "role": "tool",
            "content": tool_content,
            "tool_call_id": call.id,
            "agent_id": request.agent_id,
            "metadata": {"authorKind":"tool","roomId":session.get("roomId"),"executionId":execution_id},
        }))
        .ok_or("도구 결과 메시지를 저장할 수 없습니다.")?;
    state.bus.message(&saved);
    if let Some(frame)=native_frame.as_mut(){
        let observed=if ok{Ok(result.to_string())}else{Err(result.to_string())};
        dominion_core::journal::finish(&mut frame.native,&native_call,&observed,false)?;
        frame.receipts.push(dominion_core::hosted::Receipt{id:native_call.id,tool:native_call.name,arguments:native_call.arguments,success:ok,result:result.clone(),step:round});
        frame.native.status=if ok{dominion_core::domain::RunStatus::Completed}else{dominion_core::domain::RunStatus::Failed};
        dominion_core::hosted::Journal::save(&native_journal,frame)?;
    }
    Ok((result, ok, tool_content))
}

pub(super) fn record_soft_tool_error(
    state: &AppState,
    tx: &UnboundedSender<Value>,
    session_id: &str,
    request: &TurnRequest,
    execution_id: &str,
    name: &str,
    call_id: &str,
    error: &str,
    metrics: &std::sync::Mutex<Metrics>,
) {
    let _ = tx.send(json!({"type":"tool_error","name":name,"id":call_id,"error":error}));
    metrics.lock().unwrap().tool_calls += 1;
    state.store.audit("tool.soft_error", json!({"tool":name,"error":error,"executionId":execution_id}));
    if let Some(saved) = state.store.write_message(&json!({
        "session_id":session_id,"role":"tool","content":json!({"error":error}).to_string(),
        "tool_call_id":call_id,"agent_id":request.agent_id,
        "metadata":{"authorKind":"tool","executionId":execution_id},
    })) {
        state.bus.message(&saved);
    }
}

