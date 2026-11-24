//! Cross-domain strategy composition from host-verified evidence.
//! Produces candidates only, not execution permission or policy promotion.
use serde_json::{json,Value};

pub fn domain(tool: &str) -> &'static str {
    if tool.starts_with("github") {
        "source_control"
    } else if tool.starts_with("workspace_") || tool.starts_with("file_") {
        "filesystem"
    } else if tool.starts_with("web_") || tool.starts_with("http_") {
        "http_api"
    } else if tool.starts_with("memory_") {
        "memory"
    } else if tool.starts_with("shell") || tool.starts_with("terminal") {
        "system"
    } else if tool.contains("sql") || tool.contains("database") {
        "database"
    } else if tool.starts_with("mcp__") {
        "external_tool"
    } else {
        "generic"
    }
}

pub fn is_mutation(tool: &str, arguments: &Value) -> bool {
    if tool == "github_api" {
        return arguments["method"].as_str().is_some_and(|method| {
            matches!(method.to_ascii_uppercase().as_str(), "PUT" | "POST" | "PATCH" | "DELETE")
        });
    }
    [
        "write", "update", "delete", "remove", "patch", "put", "post", "create", "move",
        "rename", "edit", "commit", "push", "archive", "insert", "replace", "exec",
    ]
    .iter()
    .any(|word| tool.to_ascii_lowercase().contains(word))
}

pub fn is_observation(tool: &str, arguments: &Value) -> bool {
    if tool == "github_api" {
        return arguments["method"].as_str().is_none_or(|method| method.eq_ignore_ascii_case("GET"));
    }
    [
        "read", "list", "get", "status", "head", "search", "inspect", "show", "diff",
        "fetch", "verify", "check", "query", "describe",
    ]
    .iter()
    .any(|word| tool.to_ascii_lowercase().contains(word))
}

pub fn observed_success(value: &Value) -> bool {
    value.is_object()
        && !value.as_object().is_some_and(|object| object.is_empty())
        && value["outcomeUnknown"].as_bool() != Some(true)
        && value["isError"].as_bool() != Some(true)
        && value["success"].as_bool() != Some(false)
        && value["error"].is_null()
        && value.get("exitCode").is_none_or(|code| code.as_i64() == Some(0))
        && value
            .get("status")
            .and_then(Value::as_u64)
            .is_none_or(|status| status < 400)
}

pub fn principle_text(kind: &str, class: &str) -> &'static str {
    match (kind, class) {
        ("verify-before-mutate", "stale_state") => {
            "변경 전에 대상의 현재 상태와 버전을 읽고, 기대한 revision과 일치하는지 확인한 뒤 수정한다."
        }
        ("verify-before-mutate", "not_found") => {
            "행동 전에 대상이 실제로 존재하는지 확인하고, 없으면 생성·경로·대상을 먼저 확정한다."
        }
        ("verify-before-mutate", _) => {
            "변경 전에 읽기·상태 확인 도구로 현재 상태를 관찰한 뒤 수정한다."
        }
        _ => "실패한 작업의 조건을 먼저 관찰하고, 확인된 조건에 맞춰 다음 행동을 바꾼다.",
    }
}

pub fn principle_id(kind: &str, class: &str) -> String {
    format!(
        "principle-{}",
        crate::memory::stable_id(&[kind, class])
    )
}

const DOMAINS: &[&str] = &[
    "filesystem",
    "source_control",
    "http_api",
    "database",
    "external_tool",
    "system",
];

/// Capabilities are described by role, not by a concrete argument. A domain
/// only supports a composition when it can observe a version, mutate, and
/// independently verify. `available:false` means the principle may still be
/// retrieved as advice, but it must not become an executable strategy there.
fn capability_bindings(domain: &str) -> Value {
    match domain {
        "filesystem" => json!({
            "observe": ["workspace_read", "file_read"],
            "mutate": ["workspace_write", "file_write"],
            "verify": {"kind": "sha256_readback", "available": true},
            "version_source": "sha256",
            "conditional_update": "content_addressed_write",
        }),
        "source_control" => json!({
            "observe": ["github_api:GET"],
            "mutate": ["github_api:PUT", "github_api:PATCH", "github_api:DELETE"],
            "verify": {"kind": "read_after_write", "available": true},
            "version_source": "git_object_sha_or_etag",
            "conditional_update": "sha_conditional_update",
        }),
        "http_api" => json!({
            "observe": ["http_fetch"],
            "mutate": ["http_request"],
            "verify": {"kind": "read_after_write", "available": false},
            "version_source": "etag_or_last_modified",
            "conditional_update": "if_match",
        }),
        "database" => json!({
            "observe": ["sql_query"],
            "mutate": ["sql_exec"],
            "verify": {"kind": "read_after_write", "available": false},
            "version_source": "row_version_or_updated_at",
            "conditional_update": "compare_and_set",
        }),
        "external_tool" => json!({
            "observe": ["mcp__*__read", "mcp__*__list"],
            "mutate": ["mcp__*__write", "mcp__*__update"],
            "verify": {"kind": "server_declared_readback", "available": false},
            "version_source": "server_declared_etag",
            "conditional_update": "server_declared_idempotency_key",
        }),
        _ => json!({
            "observe": ["shell_exec", "terminal_exec"],
            "mutate": ["shell_exec", "terminal_exec"],
            "verify": {"kind": "artifact_or_independent_test", "available": false},
            "version_source": "none",
            "conditional_update": "none",
        }),
    }
}

fn failure_cause(class: &str) -> &'static str {
    match class {
        "stale_state" => "변경 대상의 현재 버전과 요청이 가정한 버전이 달랐다.",
        "not_found" => "대상 경로나 리소스가 실제로 존재하지 않았다.",
        "timeout" => "호출이 끝났지만 외부 효과의 상태가 확정되지 않았다.",
        "permission_denied" => "호출자에게 해당 변경 권한이 없었다.",
        "rate_limited" => "제공자 쿼터가 소진되어 관찰이나 변경이 거절됐다.",
        "invalid_inputs" => "인자가 도구 스키마나 대상의 형식과 맞지 않았다.",
        _ => "실패 원인이 관찰되지 않았다. 이 원리는 자동 적용하지 않는다.",
    }
}

fn failure_layer(class: &str) -> &'static str {
    match class {
        "timeout" => "execution",
        "permission_denied" | "rate_limited" => "authority",
        "invalid_inputs" => "tool_arguments",
        "stale_state" | "not_found" => "state",
        _ => "unknown",
    }
}

/// Structured form of a principle. This keeps the original one-line text for
/// context injection, but adds the fields a composer needs: conditions,
/// exclusions, typed procedure roles, expected evidence and cost.
pub fn synthesize(kind: &str, class: &str, failed_tool: &str, selected_tool: &str) -> Value {
    let source_domain = domain(failed_tool);
    let bindings = capability_bindings(source_domain);
    let condition = match class {
        "stale_state" => json!({
            "kind": "version_match",
            "expr": "beforeVersion is known and equals the version just observed",
            "required": true,
        }),
        "not_found" => json!({
            "kind": "existence",
            "expr": "target existence is confirmed before creating or retrying",
            "required": true,
        }),
        "timeout" => json!({
            "kind": "effect_state",
            "expr": "previous attempt has no confirmed effect, or has been inspected",
            "required": true,
        }),
        _ => json!({
            "kind": "observed_failure",
            "expr": "the same structural failure class is observed on the current task",
            "required": true,
        }),
    };
    let mut exclusions = vec![
        "대상의 현재 버전을 읽을 수단이 없다.",
        "변경이 관찰한 버전에 조건부로 묶여 있지 않다.",
        "치환 대상이 여러 개라 하나로 특정할 수 없다.",
        "권한이 거절되었거나 요청 자체가 승인되지 않았다.",
        "비가역적 외부 효과이고 idempotency key나 조회 수단이 없다.",
    ];
    if class == "timeout" {
        exclusions.push("이전 시도의 효과 상태를 조회하기 전에는 재전송하지 않는다.");
    }
    json!({
        "schemaVersion": 2,
        "kind": kind,
        "failureCause": failure_cause(class),
        "failureLayer": failure_layer(class),
        "conditions": [
            condition,
            json!({
                "kind": "mutation",
                "expr": "the current tool mutates the same resource_key role",
                "required": true,
            }),
            json!({
                "kind": "version_capability",
                "expr": "the target exposes a revision, etag, sha or row version",
                "required": true,
            }),
        ],
        "exclusions": exclusions,
        "procedure": [
            {
                "step": 1, "operator": "observe", "role": "read_current_version",
                "bindings": bindings["observe"],
                "produces": ["observedVersion", "resourceKey"],
            },
            {
                "step": 2, "operator": "compare", "role": "match_expected_version",
                "requires": ["observedVersion is not null"],
                "onMismatch": "reobserve_once_then_stop",
            },
            {
                "step": 3, "operator": "mutate", "role": "conditional_write",
                "bindings": bindings["mutate"],
                "requires": ["compare matched"],
                "versionSource": bindings["version_source"],
                "conditionalUpdate": bindings["conditional_update"],
            },
            {
                "step": 4, "operator": "verify", "role": "independent_readback",
                "bindings": bindings["verify"],
                "acceptsModelClaim": false,
            },
        ],
        "expectedEvidence": {
            "kind": "version_equals",
            "description": "관찰한 버전과 변경 후 버전이 일치하거나, 되읽은 내용의 해시가 요청과 일치한다.",
            "independent": true,
        },
        "verification": {
            "owner": "harness",
            "method": bindings["verify"]["kind"],
            "modelClaimAccepted": false,
        },
        "cost": {"modelCalls": 0, "extraToolCalls": 1, "estTokens": 0},
        "provenance": {
            "sourceTool": failed_tool,
            "recoveryTool": selected_tool,
            "sourceDomain": source_domain,
            "capabilities": bindings,
        },
        "counterexamples": [],
        "autoApply": false,
    })
}

/// Composes an executable strategy candidate from a principle only when the
/// evidence is independent and the domain exposes observe/mutate/verify. It
/// never marks the candidate promotable: a paired holdout must run first.
pub fn compose(principle: &Value, now: &str) -> Option<Value> {
    let principle_id = principle["id"].as_str()?;
    let evidence = principle["evidenceExecutionIds"]
        .as_array()
        .map(Vec::len)
        .unwrap_or(0);
    let cases = principle["supportingCases"]
        .as_array()
        .map(Vec::len)
        .unwrap_or(0);
    let counterexamples = principle["counterexamples"]
        .as_array()
        .map(Vec::len)
        .unwrap_or(0);
    if evidence < 2 || cases < 2 || counterexamples >= cases + 1 {
        return None;
    }
    let mut supported = Vec::new();
    let mut unavailable = Vec::new();
    for domain in DOMAINS {
        let bindings = capability_bindings(domain);
        let executable = bindings["mutate"]
            .as_array()
            .is_some_and(|items| !items.is_empty())
            && bindings["verify"]["available"].as_bool() == Some(true);
        if executable {
            supported.push(json!({
                "domain": domain,
                "observe": bindings["observe"],
                "mutate": bindings["mutate"],
                "verify": bindings["verify"],
                "versionSource": bindings["version_source"],
            }));
        } else {
            unavailable.push(json!({
                "domain": domain,
                "reason": "관찰·변경·독립 검증 중 하나 이상이 도구 카탈로그에 없다.",
            }));
        }
    }
    if supported.is_empty() {
        return None;
    }
    let revision = crate::memory::stable_id(&[
        principle_id,
        principle["updatedAt"].as_str().unwrap_or(""),
    ]);
    let id = format!("strategy-composition-{revision}");
    Some(json!({
        "id": id,
        "schemaVersion": 2,
        "kind": "strategy_composition",
        "status": "candidate",
        "promotionEligible": false,
        "promotionReason": "paired holdout evidence가 없으므로 운영 정책으로 승격하지 않는다.",
        "principleId": principle_id,
        "parentRevision": revision,
        "procedure": principle["procedure"],
        "conditions": principle["conditions"],
        "exclusions": principle["exclusions"],
        "expectedEvidence": principle["expectedEvidence"],
        "supportedDomains": supported,
        "unavailableDomains": unavailable,
        "evidence": {
            "executionIds": principle["evidenceExecutionIds"],
            "supportingCases": principle["supportingCases"],
            "supportingModels": principle["supportingModels"],
            "supportingAgents": principle["supportingAgents"],
            "counterexamples": principle["counterexamples"],
            "evidenceGrade": principle["evidenceGrade"],
        },
        "costBound": {"extraToolCalls": 1, "modelCalls": 0},
        "experiment": {
            "status": "not_run",
            "baseline": "existing path",
            "treatment": ["observe_version", "compare", "conditional_mutate", "independent_readback"],
            "gates": {
                "minSuccessGain": 0.10,
                "tokenRatioMax": 1.25,
                "forbidDuplicateEffect": true,
                "forbidAuthorityWidening": true,
            },
        },
        "createdAt": now,
    }))
}


#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn cross_domain_composition_requires_evidence_and_cannot_self_promote(){
        let mut evidence=json!({"id":"p","updatedAt":"fixed","evidenceExecutionIds":["r1"],"supportingCases":["c1"]});
        assert!(compose(&evidence,"fixed").is_none());
        evidence["evidenceExecutionIds"]=json!(["r1","r2"]);
        evidence["supportingCases"]=json!(["c1","c2"]);
        let proposal=compose(&evidence,"fixed").unwrap();
        assert_eq!(proposal["promotionEligible"],false);
        assert_eq!(proposal["status"],"candidate");
        assert!(proposal["supportedDomains"].as_array().unwrap().iter().any(|d|d["domain"]=="filesystem"));
        assert!(proposal["unavailableDomains"].as_array().unwrap().iter().any(|d|d["domain"]=="database"));
    }
}
