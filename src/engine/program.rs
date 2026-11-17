//! Typed Program IR (design 2.4, section 11-3).
//! A single runtime interprets the nodes and persists its observations,
//! completed operation IDs, branch inputs and per-loop iteration positions.
use serde_json::{json, Value};

pub const SUPPORTED_REVISION: u32 = 1;

pub fn validate(program: &Value) -> Result<Value, String> {
    let revision = program["revision"]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or("program.revision is required")?
        .to_string();
    let nodes = program["nodes"]
        .as_array()
        .ok_or("program.nodes must be an array")?;
    if nodes.is_empty() || nodes.len() > 64 {
        return Err("program.nodes must contain 1..64 nodes".into());
    }
    let mut identifiers = std::collections::HashSet::new();
    let mut total = 0usize;
    for node in nodes {
        validate_node(node)?;
        validate_identifiers(node, &mut identifiers)?;
        count_program_nodes(node, 0, &mut total, 128)?;
    }
    Ok(json!({"revision": revision, "nodes": nodes}))
}

/// Prevents different tool nodes (including nested branches and loop bodies)
/// from sharing a completed-operation identity after a restart.
fn validate_identifiers(
    node: &Value,
    identifiers: &mut std::collections::HashSet<String>,
) -> Result<(), String> {
    // Nested sequence/parallel/loop nodes must pass the same admission as roots.
    validate_node(node)?;
    let kind = node["type"].as_str().unwrap_or("");
    if let Some(id) = node["id"].as_str().filter(|id| !id.is_empty()) {
        if !identifiers.insert(id.to_string()) {
            return Err(format!("duplicate program node id: {id}"));
        }
    } else if matches!(kind, "call_tool" | "branch" | "loop" | "observe" | "delegate") {
        return Err(format!("{kind}.id is required for durable execution"));
    }
    for field in ["nodes", "then", "else", "body"] {
        for child in node[field].as_array().into_iter().flatten() {
            validate_identifiers(child, identifiers)?;
        }
    }
    Ok(())
}

fn validate_node(node: &Value) -> Result<(), String> {
    match node["type"].as_str().unwrap_or("") {
        "observe" => {
            if node["id"].as_str().is_none_or(str::is_empty) {
                return Err("observe.id is required".into());
            }
            Ok(())
        }
        "call_tool" => {
            if node["tool"].as_str().is_none_or(str::is_empty) {
                return Err("call_tool.tool is required".into());
            }
            if !node["inputs"].is_object() {
                return Err("call_tool.inputs must be an object".into());
            }
            Ok(())
        }
        "branch" => {
            if node["then"].as_array().is_none() || node["else"].as_array().is_none() {
                return Err("branch.then/else must be arrays".into());
            }
            for child in node["then"].as_array().into_iter().flatten()
                .chain(node["else"].as_array().into_iter().flatten())
            {
                validate_node(child)?;
            }
            Ok(())
        }
        "loop" => {
            if node["body"].as_array().is_none() {
                return Err("loop.body must be an array".into());
            }
            for child in node["body"].as_array().into_iter().flatten() {
                validate_node(child)?;
            }
            Ok(())
        }
        "sequence" | "parallel" => {
            if node["nodes"].as_array().is_none() {
                return Err(format!("{}.nodes must be an array", node["type"]));
            }
            for child in node["nodes"].as_array().into_iter().flatten() {
                validate_node(child)?;
            }
            Ok(())
        }
        "verify" => {
            let checks = node["checks"].as_array()
                .filter(|checks| !checks.is_empty() && checks.len() <= 32)
                .ok_or("verify.checks requires 1..32 predicates")?;
            if checks.iter().any(|check| !check["predicate"].is_object()) {
                return Err("verify checks require an explicit predicate object".into());
            }
            Ok(())
        }
        "model_decision" => Ok(()),
        "delegate" => {
            if node["agentId"].as_str().is_none_or(str::is_empty)
                || node["prompt"].as_str().is_none_or(str::is_empty)
            {
                return Err("delegate.agentId/prompt are required".into());
            }
            Ok(())
        }
        other => Err(format!("unsupported program node type: {other}")),
    }
}

pub fn new_checkpoint(program: &Value) -> Value {
    json!({
        "schemaVersion": SUPPORTED_REVISION,
        "programRevision": program["revision"],
        "status": "running",
        "frameStack": [],
        "nextIndex": 0,
        "resolvedInputs": {},
        "completedOperationIds": [],
        "pendingOperationIds": [],
        "loopProgress": {},
        "observationRefs": {},
        "adaptationRevision": 0,
    })
}

/// Resolves a literal or `{from, path}` input against checkpoint inputs.
pub fn resolve_inputs(inputs: &Value, resolved: &Value) -> Result<Value, String> {
    let mut out = serde_json::Map::new();
    for (key, value) in inputs.as_object().cloned().unwrap_or_default() {
        if let Some(source) = value.get("from").and_then(|value| value.as_str()) {
            let base = resolved
                .get(source)
                .ok_or_else(|| format!("input {key} references unknown observation {source}"))?;
            let selected = match value.get("path").and_then(|value| value.as_str()) {
                Some(path) => base.pointer(path).cloned().unwrap_or(Value::Null),
                None => base.clone(),
            };
            out.insert(key, selected);
        } else {
            out.insert(key, value);
        }
    }
    Ok(Value::Object(out))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PredicateVerdict {
    True,
    False,
    Unknown,
}

impl PredicateVerdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::True => "true",
            Self::False => "false",
            Self::Unknown => "unknown",
        }
    }

    pub fn is_true(self) -> bool {
        matches!(self, Self::True)
    }
}

fn predicate_observation(predicate: &Value, resolved: &Value) -> Result<Option<Value>, String> {
    let source = predicate["from"]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or("predicate.from is required")?;
    let Some(base) = resolved.get(source) else {
        return Ok(None);
    };
    if base.is_null() {
        return Ok(None);
    }
    match predicate["path"].as_str() {
        Some(path) => Ok(base.pointer(path).cloned()),
        None => Ok(Some(base.clone())),
    }
}

/// Three-valued predicate evaluation. Missing observation fields are Unknown,
/// not False, so they can never refute a hypothesis.
pub fn predicate_verdict(
    predicate: &Value,
    resolved: &Value,
) -> Result<PredicateVerdict, String> {
    match predicate["type"].as_str().unwrap_or("") {
        "exists" => {
            let source = predicate["from"].as_str().unwrap_or("");
            let Some(base) = resolved.get(source) else {
                // `exists` is a scope-complete check over the resolved map.
                return Ok(PredicateVerdict::False);
            };
            if base.is_null() {
                return Ok(PredicateVerdict::False);
            }
            match predicate["path"].as_str() {
                Some(path) => Ok(if base.pointer(path).is_some() {
                    PredicateVerdict::True
                } else {
                    PredicateVerdict::False
                }),
                None => Ok(PredicateVerdict::True),
            }
        }
        "equals" => {
            let Some(actual) = predicate_observation(predicate, resolved)? else {
                return Ok(PredicateVerdict::Unknown);
            };
            Ok(if actual == predicate["value"] {
                PredicateVerdict::True
            } else {
                PredicateVerdict::False
            })
        }
        "compare" => {
            let Some(actual) = predicate_observation(predicate, resolved)? else {
                return Ok(PredicateVerdict::Unknown);
            };
            let (Some(left), Some(right)) =
                (actual.as_f64(), predicate["value"].as_f64())
            else {
                return Ok(PredicateVerdict::Unknown);
            };
            let verdict = match predicate["op"].as_str().unwrap_or("") {
                "gt" => left > right,
                "gte" => left >= right,
                "lt" => left < right,
                "lte" => left <= right,
                other => return Err(format!("unsupported compare op: {other}")),
            };
            Ok(if verdict {
                PredicateVerdict::True
            } else {
                PredicateVerdict::False
            })
        }
        "not" => Ok(match predicate_verdict(&predicate["predicate"], resolved)? {
            PredicateVerdict::True => PredicateVerdict::False,
            PredicateVerdict::False => PredicateVerdict::True,
            PredicateVerdict::Unknown => PredicateVerdict::Unknown,
        }),
        "all" => {
            let mut unknown = false;
            for item in predicate["predicates"].as_array().into_iter().flatten() {
                match predicate_verdict(item, resolved)? {
                    PredicateVerdict::False => return Ok(PredicateVerdict::False),
                    PredicateVerdict::Unknown => unknown = true,
                    PredicateVerdict::True => {}
                }
            }
            Ok(if unknown {
                PredicateVerdict::Unknown
            } else {
                PredicateVerdict::True
            })
        }
        "any" => {
            let mut unknown = false;
            for item in predicate["predicates"].as_array().into_iter().flatten() {
                match predicate_verdict(item, resolved)? {
                    PredicateVerdict::True => return Ok(PredicateVerdict::True),
                    PredicateVerdict::Unknown => unknown = true,
                    PredicateVerdict::False => {}
                }
            }
            Ok(if unknown {
                PredicateVerdict::Unknown
            } else {
                PredicateVerdict::False
            })
        }
        other => Err(format!("unsupported predicate type: {other}")),
    }
}

/// One execution-backed verdict for a set of checks. The three-valued
/// predicate kernel is shared by Program branching, hypothesis adaptation and
/// the verification gate. Unknown is never counted as passing or refuted.
pub fn evaluate_verification_checks(
    checks: &[Value],
    resolved: &Value,
) -> Result<Value, String> {
    if checks.is_empty() || checks.len() > 32 {
        return Err("verification needs 1..32 explicit checks".into());
    }
    let mut results = Vec::with_capacity(checks.len());
    let mut failed = false;
    let mut unknown = false;
    for check in checks {
        let predicate = check.get("predicate").filter(|p| p.is_object())
            .ok_or("verification check needs a predicate object")?;
        let verdict = predicate_verdict(predicate, resolved)?;
        failed |= verdict == PredicateVerdict::False;
        unknown |= verdict == PredicateVerdict::Unknown;
        results.push(json!({"predicate":predicate,"verdict":verdict.as_str()}));
    }
    Ok(json!({
        "ok": !failed && !unknown,
        "failed": failed,
        "unknown": unknown,
        "status": if failed { "failed" } else if unknown { "unknown" } else { "passed" },
        "checks": results,
    }))
}

/// Boolean convenience for callers that can treat Unknown as non-satisfied.
/// Branch/Verify must use `predicate_verdict` so Unknown is not silently False.
pub fn predicate_holds(predicate: &Value, resolved: &Value) -> Result<bool, String> {
    Ok(predicate_verdict(predicate, resolved)?.is_true())
}

/// Extracts the first brace-balanced JSON object from model prose.
pub fn parse_json_object(raw: &str) -> Option<Value> {
    let bytes = raw.as_bytes();
    for (start, _) in raw.match_indices('{') {
        let mut depth = 0i32;
        let mut in_string = false;
        let mut escape = false;
        for index in start..bytes.len() {
            let byte = bytes[index];
            if in_string {
                if escape {
                    escape = false;
                } else if byte == b'\\' {
                    escape = true;
                } else if byte == b'"' {
                    in_string = false;
                }
                continue;
            }
            match byte {
                b'"' => in_string = true,
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        if let Ok(value) = serde_json::from_str::<Value>(&raw[start..=index]) {
                            if value.is_object() {
                                return Some(value);
                            }
                        }
                        break;
                    }
                }
                _ => {}
            }
        }
    }
    None
}

/// Bound the *whole* executable tree, not only its root nodes. A nested
/// sequence with 64 children per level must not bypass the program budget.
fn count_program_nodes(
    node: &Value,
    depth: usize,
    total: &mut usize,
    limit: usize,
) -> Result<(), String> {
    if depth > 16 {
        return Err("program nesting is deeper than 16 levels".into());
    }
    *total += 1;
    if *total > limit {
        return Err(format!(
            "program has more than {limit} executable nodes (including nested nodes)"
        ));
    }
    for field in ["nodes", "then", "else", "body"] {
        for child in node[field].as_array().into_iter().flatten() {
            count_program_nodes(child, depth + 1, total, limit)?;
        }
    }
    Ok(())
}

/// An operation already completed inside a loop has a scoped durable id:
/// loop:iteration:child. Editing that child, or any enclosing container, could
/// silently change the meaning of a recorded effect on replay.
fn touches_completed(node: &Value, completed: &std::collections::BTreeSet<String>) -> bool {
    if let Some(id) = node["id"].as_str() {
        if completed.iter().any(|done| {
            done == id || done.strip_suffix(id)
                .is_some_and(|prefix| prefix.ends_with(':'))
        }) {
            return true;
        }
    }
    ["nodes", "then", "else", "body"].iter().any(|field| {
        node[*field].as_array().is_some_and(|children| {
            children.iter().any(|child| touches_completed(child, completed))
        })
    })
}

/// Walk the original tree so an existing nested node can actually be repaired.
/// A replacement for a nonexistent ID is rejected, not silently "accepted".
fn rewrite_pending_node(
    node: &Value,
    replacements: &std::collections::BTreeMap<String, Value>,
    completed: &std::collections::BTreeSet<String>,
    found: &mut std::collections::BTreeSet<String>,
) -> Result<Value, String> {
    let id = node["id"].as_str().unwrap_or("");
    if let Some(replacement) = replacements.get(id) {
        if touches_completed(node, completed) {
            return Err(format!(
                "cannot replace node {id}: it or a descendant has completed durable effects"
            ));
        }
        found.insert(id.to_owned());
        return Ok(replacement.clone());
    }
    let mut rewritten = node.clone();
    for field in ["nodes", "then", "else", "body"] {
        if let Some(children) = node[field].as_array() {
            rewritten[field] = Value::Array(children.iter()
                .map(|child| rewrite_pending_node(child, replacements, completed, found))
                .collect::<Result<Vec<_>, _>>()?);
        }
    }
    Ok(rewritten)
}

/// Validate tool authority and data dependencies through *every* control-flow
/// branch. Parallel siblings cannot consume each other's uncommitted outputs;
/// a value created by only one side of a branch is not guaranteed after it.
fn verify_revision_flow(
    nodes: &[Value],
    known: &mut std::collections::BTreeSet<String>,
    allowed_tools: &std::collections::HashSet<&str>,
) -> Result<(), String> {
    for node in nodes {
        let kind = node["type"].as_str().unwrap_or("");
        let id = node["id"].as_str().unwrap_or("");
        match kind {
            "call_tool" | "delegate" => {
                let tool = if kind == "delegate" {
                    "delegate_task"
                } else {
                    node["tool"].as_str().unwrap_or("")
                };
                if !allowed_tools.contains(tool) {
                    return Err(format!("tool {tool} is not authorized in this run"));
                }
                for (key, input) in node["inputs"].as_object().into_iter().flat_map(|m| m.iter()) {
                    if let Some(source) = input.get("from").and_then(Value::as_str) {
                        if !known.contains(source) {
                            return Err(format!(
                                "input {key} references unavailable observation {source}"
                            ));
                        }
                    }
                }
                known.insert(node["saveAs"].as_str().filter(|s| !s.is_empty())
                    .unwrap_or(id).to_owned());
                known.insert("last".to_string());
            }
            "observe" => {
                known.insert(id.to_owned());
            }
            "sequence" => {
                verify_revision_flow(node["nodes"].as_array().ok_or("sequence nodes missing")?,
                    known, allowed_tools)?;
            }
            "parallel" => {
                let original = known.clone();
                let mut generated = std::collections::BTreeSet::new();
                for child in node["nodes"].as_array().ok_or("parallel nodes missing")? {
                    let mut isolated = original.clone();
                    verify_revision_flow(std::slice::from_ref(child), &mut isolated, allowed_tools)?;
                    generated.extend(isolated.difference(&original).cloned());
                }
                known.extend(generated);
                known.insert("last".to_owned());
            }
            "branch" => {
                let original = known.clone();
                let mut yes = original.clone();
                let mut no = original;
                verify_revision_flow(node["then"].as_array().ok_or("branch.then missing")?,
                    &mut yes, allowed_tools)?;
                verify_revision_flow(node["else"].as_array().ok_or("branch.else missing")?,
                    &mut no, allowed_tools)?;
                *known = yes.intersection(&no).cloned().collect();
            }
            "loop" => {
                let mut iteration = known.clone();
                verify_revision_flow(node["body"].as_array().ok_or("loop.body missing")?,
                    &mut iteration, allowed_tools)?;
                // A loop can execute zero times: body outputs are not guaranteed.
            }
            "verify" => { known.insert("lastVerify".to_owned()); }
            "model_decision" => {}
            other => return Err(format!("unsupported program node type: {other}")),
        }
    }
    Ok(())
}

/// Atomically admit only a measurable, authorized repair. This preserves
/// completed effects on the exact live Program IR, including nested branches
/// and loop-scoped operations; rejected repairs never touch the checkpoint.
pub fn apply_revision_proposal(
    program: &Value,
    checkpoint: &Value,
    proposal: &Value,
    available_tools: &[String],
    max_nodes: usize,
    repair_mode: &str,
) -> Result<Value, String> {
    validate(program)?;
    let base = proposal["baseRevision"].as_str()
        .filter(|value| !value.is_empty())
        .ok_or("proposal.baseRevision is required")?;
    if Some(base) != program["revision"].as_str() {
        return Err(format!(
            "proposal baseRevision {base} does not match current revision {}",
            program["revision"]
        ));
    }
    if proposal["reason"].as_str().is_none_or(str::is_empty) {
        return Err("proposal.reason is required".into());
    }
    if checkpoint["pendingOperationIds"].as_array()
        .is_some_and(|pending| !pending.is_empty())
    {
        return Err("cannot revise a program while operations are in flight".into());
    }
    let completed = checkpoint["completedOperationIds"].as_array()
        .into_iter().flatten().filter_map(Value::as_str)
        .map(str::to_owned).collect::<std::collections::BTreeSet<_>>();
    let mut replacements = std::collections::BTreeMap::new();
    if repair_mode != "append" {
        for node in proposal["replacePendingNodes"].as_array().into_iter().flatten() {
            let id = node["id"].as_str().filter(|value| !value.is_empty())
                .ok_or("replacement node needs an id")?;
            validate_node(node)?;
            if replacements.insert(id.to_owned(), node.clone()).is_some() {
                return Err(format!("duplicate replacement node {id}"));
            }
        }
    }
    let additions = proposal["addNodes"].as_array().cloned().unwrap_or_default();
    if replacements.is_empty() && additions.is_empty() {
        return Err("proposal does not alter any pending operation".into());
    }
    let mut found = std::collections::BTreeSet::new();
    let mut nodes = program["nodes"].as_array().ok_or("program.nodes missing")?
        .iter().map(|node| rewrite_pending_node(node, &replacements, &completed, &mut found))
        .collect::<Result<Vec<_>, _>>()?;
    for id in replacements.keys() {
        if !found.contains(id) {
            return Err(format!("replacement node {id} does not exist in the current program"));
        }
    }
    for node in additions {
        validate_node(&node)?;
        nodes.push(node);
    }
    let revised = json!({"revision":format!("{base}:rev"),"nodes":nodes});
    validate(&revised)?;
    let mut total = 0usize;
    for node in revised["nodes"].as_array().into_iter().flatten() {
        count_program_nodes(node, 0, &mut total, max_nodes)?;
    }
    let allowed: std::collections::HashSet<&str> =
        available_tools.iter().map(String::as_str).collect();
    let mut known: std::collections::BTreeSet<String> = checkpoint["resolvedInputs"]
        .as_object().map(|map| map.keys().cloned().collect()).unwrap_or_default();
    verify_revision_flow(revised["nodes"].as_array().ok_or("revised nodes missing")?,
        &mut known, &allowed)?;

    Ok(json!({
        "revision": format!("{}:rev:{}", base,
            crate::engine::memory::stable_id(&[&proposal.to_string()])),
        "nodes": revised["nodes"],
        "appliedProposal": proposal,
        "parentRevision": base,
    }))
}

pub fn checkpoint_skip_completed(checkpoint: &Value, node_id: &str) -> bool {
    checkpoint["completedOperationIds"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|value| value.as_str() == Some(node_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn program() -> Value {
        json!({
            "revision":"p1",
            "nodes":[
                {"type":"call_tool","id":"try","tool":"shell_exec","inputs":{"command":"exit 7"},"saveAs":"try"},
                {"type":"branch","id":"check",
                 "predicate":{"type":"equals","from":"try","path":"/exitCode","value":0},
                 "then":[{"type":"call_tool","id":"primary","tool":"workspace_write","inputs":{"path":"primary.txt","content":"PRIMARY"}}],
                 "else":[{"type":"call_tool","id":"fallback","tool":"workspace_write","inputs":{"path":"fallback.txt","content":"FALLBACK"}}]}
            ]
        })
    }

    #[test]
    fn validation_accepts_the_minimal_program_and_rejects_unknown_nodes() {
        assert!(validate(&program()).is_ok());
        assert!(validate(&json!({"revision":"p","nodes":[{"type":"nope"}]})).is_err());
        assert!(validate(&json!({"nodes":[]})).is_err());
    }

    #[test]
    fn durable_program_rejects_duplicate_and_missing_effect_ids() {
        let mut duplicate = program();
        duplicate["nodes"][1]["else"][0]["id"] = json!("try");
        assert!(validate(&duplicate).is_err());
        let mut missing = program();
        missing["nodes"][0].as_object_mut().unwrap().remove("id");
        assert!(validate(&missing).is_err());
        let mut loop_missing = program();
        loop_missing["nodes"].as_array_mut().unwrap().push(json!({
            "type":"loop","maxIterations":2,
            "predicate":{"type":"exists","from":"loop_condition"},
            "body":[{"type":"call_tool","id":"write","tool":"workspace_write","inputs":{"path":"out","content":"x"}}]
        }));
        assert!(validate(&loop_missing).is_err());
    }

    #[test]
    fn failure_predicate_selects_the_fallback_branch() {
        let resolved = json!({"try": {"exitCode": 7, "isError": true}});
        let check = &program()["nodes"][1]["predicate"];
        assert!(!predicate_holds(check, &resolved).unwrap());
        let success = json!({"try": {"exitCode": 0}});
        assert!(predicate_holds(check, &success).unwrap());
    }

    #[test]
    fn inputs_can_reference_resolved_observations() {
        let resolved = json!({"try": {"result": {"path": "out.txt"}}});
        let inputs = json!({
            "path": {"from":"try","path":"/result/path"},
            "content":"x"
        });
        assert_eq!(
            resolve_inputs(&inputs, &resolved).unwrap(),
            json!({"path":"out.txt","content":"x"})
        );
        assert!(resolve_inputs(&json!({"p":{"from":"missing"}}), &resolved).is_err());
    }

    #[test]
    fn checkpoint_records_progress_and_skips_completed_operations_on_resume() {
        let mut checkpoint = new_checkpoint(&program());
        checkpoint["completedOperationIds"] = json!(["try"]);
        checkpoint["pendingOperationIds"] = json!(["fallback"]);
        checkpoint["nextIndex"] = json!(1);
        assert!(checkpoint_skip_completed(&checkpoint, "try"));
        assert!(!checkpoint_skip_completed(&checkpoint, "fallback"));
        // The checkpoint survives a JSON round-trip (durable resume contract).
        let round_trip: Value =
            serde_json::from_str(&serde_json::to_string(&checkpoint).unwrap()).unwrap();
        assert_eq!(round_trip["programRevision"], "p1");
        assert_eq!(round_trip["nextIndex"], 1);
        assert_eq!(round_trip["pendingOperationIds"], json!(["fallback"]));
    }
    #[test]
    fn verification_is_three_valued_and_never_passes_without_evidence() {
        let checks = [json!({"predicate":{"type":"equals","from":"build",
            "path":"/exitCode","value":0}})];
        assert_eq!(evaluate_verification_checks(&checks,
            &json!({"build":{"exitCode":0}})).unwrap()["status"], "passed");
        assert_eq!(evaluate_verification_checks(&checks,
            &json!({"build":{"exitCode":7}})).unwrap()["status"], "failed");
        assert_eq!(evaluate_verification_checks(&checks,
            &json!({})).unwrap()["status"], "unknown");
        assert!(evaluate_verification_checks(&[], &json!({})).is_err());
    }

    #[test]
    fn nested_repair_changes_the_real_pending_branch_not_a_phantom_root() {
        let original = program();
        let mut checkpoint = new_checkpoint(&original);
        checkpoint["completedOperationIds"] = json!(["try"]);
        checkpoint["resolvedInputs"] = json!({"try":{"exitCode":7}});
        let proposal = json!({
            "baseRevision":"p1","reason":"fix the failed fallback",
            "replacePendingNodes":[{"type":"call_tool","id":"fallback",
                "tool":"workspace_write","inputs":{"path":"fixed.txt","content":"FIXED"}}],
            "addNodes":[]
        });
        let fixed = apply_revision_proposal(&original, &checkpoint, &proposal,
            &["shell_exec".into(),"workspace_write".into()], 64, "replace").unwrap();
        assert_eq!(fixed["nodes"][1]["else"][0]["inputs"]["path"], "fixed.txt");
        assert_eq!(fixed["nodes"][0], original["nodes"][0]);
        assert_eq!(fixed["nodes"][1]["then"], original["nodes"][1]["then"]);
    }

    #[test]
    fn repair_refuses_completed_ancestors_scoped_effects_and_forged_targets() {
        let original = program();
        let mut checkpoint = new_checkpoint(&original);
        checkpoint["completedOperationIds"] = json!(["fallback"]);
        let replace_parent = json!({"baseRevision":"p1","reason":"replace branch",
            "replacePendingNodes":[{"type":"branch","id":"check",
                "predicate":{"type":"exists","from":"try"},"then":[],"else":[]}],
            "addNodes":[]});
        assert!(apply_revision_proposal(&original, &checkpoint, &replace_parent,
            &["shell_exec".into(),"workspace_write".into()],64,"replace")
            .unwrap_err().contains("completed durable effects"));

        checkpoint["completedOperationIds"] = json!([]);
        let unknown = json!({"baseRevision":"p1","reason":"phantom",
            "replacePendingNodes":[{"type":"observe","id":"not_present"}]});
        assert!(apply_revision_proposal(&original, &checkpoint, &unknown,
            &["shell_exec".into(),"workspace_write".into()],64,"replace")
            .unwrap_err().contains("does not exist"));

        checkpoint["completedOperationIds"] = json!(["loop_a:1:fallback"]);
        let scoped = json!({"baseRevision":"p1","reason":"rewrite scoped",
            "replacePendingNodes":[{"type":"observe","id":"fallback"}]});
        assert!(apply_revision_proposal(&original, &checkpoint, &scoped,
            &["shell_exec".into(),"workspace_write".into()],64,"replace")
            .unwrap_err().contains("completed durable effects"));
    }

    #[test]
    fn repair_rejects_inflight_effects_and_hidden_tools_in_nested_nodes() {
        let original = program();
        let mut checkpoint = new_checkpoint(&original);
        let malicious = json!({"baseRevision":"p1","reason":"try an unauthorized tool",
            "replacePendingNodes":[],"addNodes":[
                {"type":"sequence","id":"new","nodes":[
                    {"type":"call_tool","id":"danger","tool":"github_api",
                     "inputs":{"method":"DELETE","path":"/repos"}}
                ]}
            ]});
        assert!(apply_revision_proposal(&original, &checkpoint, &malicious,
            &["shell_exec".into(),"workspace_write".into()],64,"replace")
            .unwrap_err().contains("not authorized"));
        checkpoint["pendingOperationIds"] = json!(["try"]);
        assert!(apply_revision_proposal(&original, &checkpoint, &malicious,
            &["shell_exec".into(),"workspace_write".into()],64,"replace")
            .unwrap_err().contains("in flight"));
    }

    #[test]
    fn repair_rejects_parallel_dependency_and_nested_node_budget_evasion() {
        let original = program();
        let checkpoint = new_checkpoint(&original);
        let dependent = json!({"baseRevision":"p1","reason":"invalid parallel dependency",
            "addNodes":[{"type":"parallel","id":"many","nodes":[
                {"type":"call_tool","id":"a","tool":"workspace_read","inputs":{"path":"a.txt"}},
                {"type":"call_tool","id":"b","tool":"workspace_read",
                    "inputs":{"path":{"from":"a","path":"/path"}}}
            ]}]});
        let allowed = ["shell_exec".into(),"workspace_write".into(),"workspace_read".into()];
        assert!(apply_revision_proposal(&original, &checkpoint, &dependent,
            &allowed,64,"replace").unwrap_err().contains("unavailable observation"));

        let many = (0..65).map(|i|json!({"type":"observe","id":format!("obs{i}")}))
            .collect::<Vec<_>>();
        let large = json!({"baseRevision":"p1","reason":"over budget",
            "addNodes":[{"type":"sequence","id":"many","nodes":many}]});
        assert!(apply_revision_proposal(&original, &checkpoint, &large,
            &allowed,64,"replace").unwrap_err().contains("executable nodes"));
        let nested_unknown = json!({"revision":"p2","nodes":[
            {"type":"sequence","nodes":[{"id":"z","type":"unknown"}]}]});
        assert!(validate(&nested_unknown).is_err());
    }

}
