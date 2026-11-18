//! Tool dispatch, effect receipts, errors and result persistence.
use super::*;

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
    let argument_strategy = crate::engine::algorithms::choose_for_agent(
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
    crate::engine::algorithms::mark_signal(state, execution_id, "tool.arguments", true);
    // Record what the effect would be before asking for approval or running it.
    // Only mutating calls need the session history scan.
    let mut effect_operations = if crate::engine::effect::is_effectful(&call.name, &arguments) {
        let resource = crate::engine::effect::resource_key(&call.name, &arguments);
        let effect_id = crate::engine::effect::effect_id(session_id, &call.name, &arguments);
        state.store.related_effect_operations(
            session_id, resource.as_deref(), effect_id.as_deref()
        )?
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
        mcp::requires_approval(state, &call.name)
    } else {
        tools::requires_approval(&call.name)
    };
    let operation_id = format!("{execution_id}:{round}:{call_index}:{}", call.id);
    let allowed = if effect_block.is_some() {
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
                "opSeq": op_seq, "origin": origin, "procedureNodeId": procedure_node,
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
    let (result, ok) = if let Some(prior) = &effect_block {
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
            "arguments": arguments, "dispatched": false,
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
        let outcome = if state.store.get("session",session_id).is_some_and(|s|
            !tools::ui_tool_allowed(
                &crate::engine::execution_scope::active_panel_policy(state,&s),
                &call.name
            )){
            Err("사용자 설정에서 비활성화한 도구입니다.".into())
        } else if state.store.get("session",session_id).is_some_and(|s|
            s["parallelReadOnly"]==true) && !tools::parallel_read_only_tool(&call.name){
            Err("병렬 조사 하위 에이전트는 쓰기·셸·위임·MCP 실행을 할 수 없습니다.".into())
        } else if call.name==capability::SEARCH {
            capability::search(state,session_id,execution_id,&arguments).await
        } else if call.name.starts_with("mcp__") {
            mcp::call(state, &call.name, &arguments).await
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
                        "id": call.id, "arguments": arguments,
                        "error": "도구가 실패 결과를 반환했습니다.", "result": value }));
                    (value, false)
                }
            }
            Err(error) if call.name.starts_with("workspace_") && error == tools::WORKSPACE_ESCAPE => {
                let error = format!("{error}. Use the relative path inside the workspace, for example bench/report.txt.");
                let _ = tx.send(json!({"type":"tool_error","name":call.name,"id":call.id,
                    "arguments":arguments,"dispatched":false,"error":error}));
                (json!({"error":error,"invalidWorkspacePath":true}), false)
            }
            Err(error) => {
                let _ = tx.send(json!({
                    "type": "tool_error",
                    "name": call.name,
                    "id": call.id,
                    "arguments": arguments,
                    "error": error,
                }));
                (json!({ "error": error }), false)
            }
        }
    } else {
        let _ = tx.send(json!({
            "type": "tool_error",
            "name": call.name,
            "id": call.id, "arguments": arguments, "dispatched": false,
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
            let operations=state.store.related_effect_operations(
                session_id, Some(resource), None
            )?;
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
    // Frozen, model-free operations have no corresponding assistant
    // tool_calls envelope. Keep their full receipts in execution_operation,
    // but do not append orphan role=tool messages to provider chat history.
    // All model-issued and ProgramIR tools retain the existing persistence.
    if origin != "deterministic_shortcut" {
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
    let _ = tx.send(json!({"type":"tool_error","name":name,"id":call_id,
        "dispatched":false,"error":error}));
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

