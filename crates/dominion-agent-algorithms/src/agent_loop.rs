//! Replaceable, versioned model-decision and agent-turn orchestration.
//! The core runtime supplies trusted ports, admission, scheduling, and receipts.
use crate::{AgentAlgorithm,StandardAlgorithm,context,tool_discovery,failure_guidance};
use dominion_agent_runtime::{self as runtime, acceptance, scheduler, schema, Action,
    BoxFuture,Limits,CriterionKind,Criterion,RunRequest,Decision,ModelReply,ModelTurn,
    Model,Tools,Evidence,Journal,DispatchResult,Dispatcher,Status,CheckResult,
    Snapshot,Receipt,Strategy,ToolSpec,AgentLoop,AgentPorts};
use futures_util::future::join_all;
use serde_json::{json,Value};
use std::collections::HashMap;

/// Default ReAct-compatible orchestration, selected by the host separately
/// from the version-pinned prompt/intent/compaction policy.
pub struct ReActLoop<'p> {pub policy:&'p dyn AgentAlgorithm}
impl AgentLoop for ReActLoop<'_> {
    fn id(&self)->&'static str {"react-v1"}
    fn execute<'a>(&'a self,ports:AgentPorts<'a>,request:RunRequest,
        checkpoint:Option<Snapshot>)->BoxFuture<'a,Result<Snapshot,String>>{
        Box::pin(async move {
            let kernel=Kernel{model:ports.model,tools:ports.tools,
                evidence:ports.evidence,journal:ports.journal};
            match checkpoint {
                Some(saved)=>kernel.resume_with_algorithm(request,saved,self.policy).await,
                None=>kernel.run_with_algorithm(request,self.policy).await,
            }
        })
    }
}

pub struct Kernel<'a> {
    pub model: &'a dyn Model,
    pub tools: &'a dyn Tools,
    pub evidence: &'a dyn Evidence,
    pub journal: &'a dyn Journal,
}
fn estimate_tokens(text: &str) -> u64 {
    // A 4-chars-per-token rule seriously undercounts Korean/CJK prompts.
    let ascii=text.chars().filter(char::is_ascii).count();
    let unicode=text.chars().count().saturating_sub(ascii);
    ascii.div_ceil(4) as u64 + unicode as u64
}
fn decode(content: &str) -> Result<Decision, String> {
    let raw = content.trim();
    // Some compatible model providers wrap structured output in a fenced
    // block. Decode only the outermost fence; never "repair" arbitrary text.
    let json_text = if let Some(fenced) = raw.strip_prefix("```") {
        let header = fenced.find('\n').ok_or("Missing fenced JSON body")?;
        fenced[header + 1..].trim().strip_suffix("```")
            .ok_or("Unterminated JSON fence")?.trim()
    } else { raw };
    let decision: Decision = serde_json::from_str(json_text)
        .map_err(|error| format!("Invalid model decision JSON: {error}"))?;
    validate_decision_plan(decision)
}
/// Native provider function calls and text-only JSON planners must satisfy
/// the identical bounded plan contract before any decision is journaled.
fn validate_decision_plan(decision: Decision) -> Result<Decision, String> {
    if decision.plan.len() > 32 || decision.plan.iter().any(|item| item.len() > 1000) {
        return Err("Plan exceeds maximum structure size".into());
    }
    Ok(decision)
}

/// Distinguish inspection from external effects so existing files or a
/// plausible model claim cannot satisfy an unperformed edit.
/// File changes and generic external actions have different evidence.
/// Running a command, opening a browser or deploying a service must not be
/// rejected just because it did not write a workspace file.
#[cfg(test)]
fn owner_requires_file_mutation(text: &str) -> bool { StandardAlgorithm.classify(text).requires_file_mutation }
#[cfg(test)]
fn owner_requires_effect(text: &str) -> bool { StandardAlgorithm.classify(text).requires_effect }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AcceptanceRun {
    Passed,
    NewFailure,
    ExistingFailure,
    OwnerChanged,
    Cancelled,
    ToolUnavailable,
    ToolBudgetExceeded,
}

impl Kernel<'_> {
    pub async fn run(&self, request: RunRequest) -> Result<Snapshot, String> {
        self.run_with_algorithm(request,&StandardAlgorithm).await
    }

    /// Replace planning policy per run without rebuilding or forking the kernel.
    /// Permissions, write-ahead receipts, evidence and budgets remain kernel-owned.
    pub async fn run_with_algorithm(&self,request:RunRequest,
        algorithm:&dyn AgentAlgorithm)->Result<Snapshot,String>{
        self.run_from(request,None,algorithm).await
    }

    /// Continue only from a committed, exact-job checkpoint. Nothing already
    /// recorded is replayed, including completed read calls and paid model
    /// generations. Unknown external effects require explicit reconciliation.
    pub async fn resume(&self, request: RunRequest, saved: Snapshot) -> Result<Snapshot, String> {
        self.resume_with_algorithm(request,saved,&StandardAlgorithm).await
    }
    pub async fn resume_with_algorithm(&self,request:RunRequest,saved:Snapshot,
        algorithm:&dyn AgentAlgorithm)->Result<Snapshot,String>{
        self.run_from(request,Some(saved),algorithm).await
    }

    async fn run_from(&self, request: RunRequest, saved: Option<Snapshot>,
        algorithm:&dyn AgentAlgorithm) -> Result<Snapshot, String> {
        request.validate()?;
        let algorithm_id=algorithm.id();
        if algorithm_id.is_empty()||algorithm_id.len()>128||!algorithm_id.bytes().all(|b|
            b.is_ascii_alphanumeric()||matches!(b,b'-'|b'_')) {
            return Err("Invalid algorithm identity".into());
        }
        let mut run = if let Some(saved) = saved {
            if saved.id != request.id || saved.instruction != request.instruction {
                return Err("Checkpoint does not match the requested task identity".into());
            }
            match saved.algorithm_id.as_deref() {
                Some(pinned) if pinned!=algorithm_id =>
                    return Err("Checkpoint algorithm differs from the requested policy; review required".into()),
                None if algorithm_id!=StandardAlgorithm.id() =>
                    return Err("Legacy checkpoint without algorithm binding cannot switch policies".into()),
                _ => {}
            }
            if !saved.in_flight.is_empty() || saved.model_in_flight {
                return Err("Checkpoint has an uncertain tool or model outcome; manual review required".into());
            }
            if saved.step > request.limits.max_steps ||
                saved.model_calls > request.limits.max_model_calls ||
                saved.receipts.len() > request.limits.max_tool_calls {
                return Err("Checkpoint exceeds the current execution limits".into());
            }
            if !matches!(saved.status, Status::Running) {
                return Ok(saved);
            }
            saved
        } else { Snapshot::from_request(&request) };
        // Algorithm identity is journaled before any model call or tool effect.
        if run.algorithm_id.is_none(){run.algorithm_id=Some(algorithm_id.to_owned());}
        let specs = self.tools.definitions();
        let mut tools = HashMap::new();
        for spec in specs {
            if !request.allowed_tools.contains(&spec.name) {
                continue;
            }
            if tools.contains_key(&spec.name) {
                return Err(format!("Ambiguous duplicate authorized tool: {}", spec.name));
            }
            tools.insert(spec.name.clone(), spec);
        }
        // Internal observation/discovery tools are reserved Rust capabilities.
        // Host declarations must not silently shadow them or be shadowed by
        // them; a collision can change routing or the effective permissions.
        for spec in context::internal_specs() {
            if tools.contains_key(&spec.name) {
                return Err(format!("Reserved kernel tool name: {}", spec.name));
            }
            tools.insert(spec.name.clone(),spec);
        }
        if tool_discovery::needed(&tools) {
            for spec in [tool_discovery::spec(),tool_discovery::call_spec()] {
                if tools.contains_key(&spec.name) {
                    return Err(format!("Reserved kernel tool name: {}", spec.name));
                }
                tools.insert(spec.name.clone(),spec);
            }
        }
        let native_wire=self.model.supports_native_tools();
        let system=algorithm.planner_instructions(native_wire,tool_discovery::needed(&tools),
            &format!("{:?}",request.strategy),&request.owner_instructions);
        let mut invalid = 0usize;
        self.journal.save(&run)?;
        let mut drive=runtime::HarnessDrive::resume(
            request.limits.max_steps,run.step)?;
        crate::harness_steps!(
            'steps: drive,
            running = true,
            cancelled = self.journal.cancelled(&request.id),
            calls = run.model_calls,
            limit = request.limits.max_model_calls,
            on_cancel = {
                run.stop(Status::Cancelled,"Owner cancelled execution");
                self.journal.save(&run)?;
                return Ok(run);
            },
            |step| {
            run.step = step + 1;
            // Compact only the model's view. Full receipts remain durable
            // and can be searched/retrieved by internal read-only tools.
            let projected=algorithm.project_context(&run.receipts,
                request.strategy,request.limits.max_result_chars);
            let current_directive=self.journal.directive(&request.id)?;
            // Tool authorization remains the full host-owned catalogue. The
            // model sees a small, deterministic view, promoted by its own
            // durable discovery receipts rather than by an extra model call.
            let failure_diagnosis=algorithm.failure_guidance(&run.receipts);
            let visible_tools=algorithm.select_tools(
                &tools,&request.instruction,&request.owner_instructions,
                current_directive.as_deref(),&run.plan,&run.receipts);
            // An algorithm only controls which authorized schemas reach the
            // model. It cannot forge a new tool, mutate a schema or widen the
            // native provider's bounded tool disclosure.
            let max_visible=if tool_discovery::needed(&tools){13}else{tools.len()};
            let mut seen_tools=std::collections::HashSet::new();
            if visible_tools.len()>max_visible || visible_tools.iter().any(|spec|{
                !seen_tools.insert(spec.name.clone()) ||
                tools.get(&spec.name).is_none_or(|trusted|
                    trusted.schema!=spec.schema || trusted.read_only!=spec.read_only ||
                    trusted.description!=spec.description)
            }) {
                run.stop(Status::NeedsReview,
                    "Algorithm attempted to expose an unauthorized or altered tool");
                self.journal.save(&run)?;
                return Ok(run);
            }
            let catalog_for_prompt=if native_wire {
                format!("The provider-native tool catalogue exposes {} authorized schemas this turn.",
                    visible_tools.len())
            } else {
                let definitions=visible_tools.iter().map(|spec|json!({
                    "name":spec.name,
                    "description":if matches!(request.strategy,Strategy::LowCost) {
                        spec.description.chars().take(120).collect::<String>()
                    } else {spec.description.clone()},
                    "parameters":spec.schema,"readOnly":spec.read_only
                })).collect::<Vec<_>>();
                format!("Advertised authorized tools for this turn: {}",
                    serde_json::to_string(&definitions).map_err(|e|e.to_string())?)
            };
            let prompt = json!({
                "userTask":request.instruction,
                "latestOwnerDirective":current_directive, "ownerInstructions":request.owner_instructions,
                "currentPlan":run.plan, "recentObservations":projected.recent,
                "importantPastFailures":projected.important_failures,
                "latestFailureDiagnosis":failure_diagnosis.as_ref()
                    .map(failure_guidance::FailureGuidance::model_view),
                "historicalReceiptIndex":projected.history_index,
                "executionDigest":projected.digest,
                "requiredAcceptanceChecks":request.checks,
                "verificationGaps":run.checks.iter().filter(|v| !v.satisfied)
                    .map(|v| &v.explanation).collect::<Vec<_>>(),
                "remainingSteps":request.limits.max_steps-step,
                "modelBudgetRemaining":request.limits.max_model_calls-run.model_calls,
                "tokenBudgetRemaining":request.limits.max_estimated_tokens.saturating_sub(run.charged_tokens),
                "toolBudgetRemaining":request.limits.max_tool_calls.saturating_sub(run.receipts.len()),
                "authorizedToolCount":tools.len(),
                "advertisedToolCount":visible_tools.len()
            }).to_string();
            // Latest owner instruction is reloaded as a higher-priority
            // instruction on every turn; stale models cannot hide it in a
            // lower-priority tool observation.
            let active_system=if let Some(directive)=current_directive.as_deref() {
                format!("{system}\n{catalog_for_prompt}\nLATEST OWNER DIRECTIVE (overrides earlier task plans): {directive}")
            }else{format!("{system}\n{catalog_for_prompt}")};
            // Model response generation is bounded by the user budget, too.
            let estimated = estimate_tokens(&active_system) + estimate_tokens(&prompt) + 1024;
            let mut model_tokens=runtime::ModelTokenBudget::new(
                request.limits.max_estimated_tokens,run.charged_tokens,
            );
            if !model_tokens.can_admit(estimated) {
                run.stop(Status::NeedsReview, "Model token budget exhausted");
                self.journal.save(&run)?;
                return Ok(run);
            }
            // The exact same write-ahead model operator controls native
            // and Chat/Room streaming; decision policy remains pluggable.
            let run_receipts=run.receipts.clone();
            let turn = match runtime::dispatch_model_turn(
                self.journal,&mut run,current_directive.as_deref(),
                || self.model.complete_with_tools(
                    &active_system,&prompt,&visible_tools,&run_receipts),
            ).await? {
                runtime::ModelAttempt::Completed(turn)=>turn,
                runtime::ModelAttempt::Cancelled=>{
                    run.stop(Status::Cancelled,"Owner cancelled before model dispatch");
                    self.journal.save(&run)?;
                    return Ok(run);
                },
                runtime::ModelAttempt::OwnerChanged=>{
                    run.checks.push(CheckResult {
                        criterion:Criterion{kind:CriterionKind::ToolSucceeded,
                            path:None,contains:None,tool:Some("owner_directive".into())},
                        satisfied:false,
                        explanation:"Owner changed the instruction before model dispatch".into(),
                    });
                    self.journal.save(&run)?;
                    continue 'steps;
                },
                runtime::ModelAttempt::Uncertain(error)=>{
                    if self.journal.cancelled(&request.id){
                        run.model_in_flight=false;
                        run.stop(Status::Cancelled,"Owner cancelled native model generation");
                    }else{
                        run.stop(Status::NeedsReview,format!("Model unavailable: {error}"));
                    }
                    self.journal.save(&run)?;
                    return Ok(run);
                }
            };
            // The model's native function calls are validated by the same
            // kernel-owned scheduler, authorization and durable checkpoints.
            let native_decision=turn.decision;
            let reply=turn.reply;
            run.model_calls += 1;
            // Persist the fact that generation ended before deciding next actions.
            // If this checkpoint fails, no actions are dispatched.
            run.model_in_flight = false;
            let actual_tokens=reply.input_tokens
                .unwrap_or_else(||estimate_tokens(&active_system)+estimate_tokens(&prompt))
                .saturating_add(reply.output_tokens
                    .unwrap_or_else(||estimate_tokens(&reply.content)));
            let within_token_budget=model_tokens.charge(actual_tokens);
            run.charged_tokens=model_tokens.charged();
            self.journal.save(&run)?;
            // The preflight reserve is an estimate, not authorization to
            // exceed the owner's actual token budget. Provider-reported usage
            // may exceed that reserve. Preserve the paid result in the durable
            // checkpoint, but do not accept a final answer or dispatch any
            // external effect from an over-budget model turn.
            if !within_token_budget {
                run.stop(Status::NeedsReview,
                    "Provider-reported token usage exceeded the owner budget; no actions dispatched");
                self.journal.save(&run)?;
                return Ok(run);
            }
            let decision = match native_decision.map_or_else(||decode(&reply.content),validate_decision_plan)
                .and_then(|decision|{algorithm.validate_plan(&decision.plan)?;Ok(decision)}) {
                Ok(d) => d,
                Err(error) => {
                    invalid += 1;
                    run.checks = vec![CheckResult {
                        criterion: Criterion {kind:CriterionKind::ToolSucceeded,path:None,contains:None,tool:Some("decision_parse".into())},
                        satisfied:false, explanation:error
                    }];
                    self.journal.save(&run)?;
                    if invalid >= 3 { break; }
                    continue;
                }
            };
            // Store the MODEL decision, before any host action or acceptance
            // outcome, so historical failures are reproducible offline.
            // This is durable evidence, never authority to re-execute effects.
            run.decision_tape.push(decision.clone());
            self.journal.save(&run)?;
            if decision.plan.len() <= 32 {
                run.plan = decision.plan.into_iter().map(|s| s.chars().take(400).collect()).collect();
            }
            // An owner correction arriving during generation invalidates the
            // stale decision before any new tool call or final claim.
            if self.journal.directive(&request.id)? != current_directive {
                run.checks = vec![CheckResult {
                    criterion: Criterion {kind:CriterionKind::ToolSucceeded,path:None,
                        contains:None,tool:Some("owner_directive".into())},
                    satisfied:false,
                    explanation:"Owner changed the instruction; replan before acting".into()
                }];
                self.journal.save(&run)?;
                continue;
            }
            // A final answer never overrides an independent acceptance failure.
            if decision.actions.is_empty() {
                if let Some(answer) = decision.answer.filter(|s| !s.trim().is_empty()) {
                    // Don't waste user-approved test executions on an edit task
                    // for which the host has not recorded a single real write.
                    // Intelligence may ask for more evidence, never waive standard checks.
                    let intent=StandardAlgorithm.classify(&request.instruction)
                        .union(algorithm.classify(&request.instruction));
                    let requires_file_mutation = intent.requires_file_mutation;
                    let requires_external_effect = intent.requires_effect;
                    let has_nonread_effect = run.receipts.iter().any(|r|r.success &&
                        tools.get(&r.tool).is_some_and(|tool| !tool.read_only));
                    let has_confirmed_write = run.receipts.iter().any(|r|r.success &&
                        r.result["written"] == true &&
                        tools.get(&r.tool).is_some_and(|tool| !tool.read_only));
                    if requires_file_mutation && !has_confirmed_write {
                        run.checks = vec![CheckResult {
                            criterion:Criterion{kind:CriterionKind::ToolSucceeded,
                                path:None,contains:None,tool:Some("confirmed_write".into())},
                            satisfied:false,
                            explanation:"No host-confirmed file modification. Use a workspace editing tool and verify its written=true receipt before finishing".into(),
                        }];
                        invalid += 1;
                        self.journal.save(&run)?;
                        if invalid >= 3 {break;}
                        continue;
                    }
                    // Verify cheap file conditions before running shell tests.
                    // No compiler invocation is needed when a required file
                    // is missing or does not contain the requested change.
                    let cheap_checks=self.verify(&request,&run,&tools).await;
                    if cheap_checks.iter().any(|check| !check.satisfied
                        && check.criterion.kind != CriterionKind::ToolSucceeded) {
                        run.checks=cheap_checks;
                        self.journal.save(&run)?;
                        continue;
                    }
                    // The model may propose the final answer, but only the
                    // Rust kernel decides when required tests run and pass.
                    // Run all missing tests within this same model step,
                    // without paying for a separate planning call per test.
                    match self.run_required_commands(
                        &request, &mut run, &tools, current_directive.as_deref(),
                    ).await? {
                        AcceptanceRun::Passed => {}
                        AcceptanceRun::NewFailure => {
                            run.checks = self.verify(&request, &run, &tools).await;
                            self.journal.save(&run)?;
                            // Hand the real failure receipt to the model for
                            // source repair; do not blindly retry the test.
                            continue;
                        }
                        AcceptanceRun::ExistingFailure => {
                            run.stop(Status::NeedsReview,
                                "Required verification previously failed; no intervening repair was observed");
                            run.checks = self.verify(&request, &run, &tools).await;
                            self.journal.save(&run)?;
                            return Ok(run);
                        }
                        AcceptanceRun::ToolBudgetExceeded => {
                            run.stop(Status::NeedsReview,
                                "Owner-required verification would exceed the durable tool-call budget");
                            self.journal.save(&run)?;
                            return Ok(run);
                        }
                        AcceptanceRun::ToolUnavailable => {
                            run.stop(Status::NeedsReview,
                                "Required shell verification is not allowed by this run's tool permissions");
                            self.journal.save(&run)?;
                            return Ok(run);
                        }
                        AcceptanceRun::OwnerChanged => {
                            self.journal.save(&run)?;
                            continue;
                        }
                        AcceptanceRun::Cancelled => {
                            run.stop(Status::Cancelled,"Owner cancelled verification");
                            self.journal.save(&run)?;
                            return Ok(run);
                        }
                    }
                    run.checks = self.verify(&request, &run, &tools).await;
                    // A lengthy real test can finish after the owner has
                    // changed the goal. Never accept a stale completion.
                    if self.journal.directive(&request.id)?.as_deref()
                        != current_directive.as_deref() {
                        self.journal.save(&run)?;
                        continue;
                    }
                    if self.journal.cancelled(&request.id) {
                        run.stop(Status::Cancelled,"Owner cancelled execution");
                        self.journal.save(&run)?;
                        return Ok(run);
                    }
                    // Both evidence sources were checked before running tests;
                    // tests still cannot serve as proof of a file edit.
                    // A preexisting file can satisfy file_exists but cannot
                    // prove a requested change. Require a genuine successful
                    // non-read tool effect for action tasks unless the owner
                    // explicitly disabled evidence checking.
                    let observed = run.receipts.iter().any(|r| r.success);
                    let all_checks = run.checks.iter().all(|c| c.satisfied);
                    let evidence_sufficient = !request.require_evidence
                        || (all_checks && (!requires_external_effect || has_nonread_effect)
                            && (!requires_file_mutation || has_confirmed_write)
                            && (observed || !request.checks.is_empty()));
                    if evidence_sufficient && run.checks.iter().all(|c| c.satisfied) {
                        run.answer = Some(answer);
                        run.status = Status::Completed;
                        self.journal.save(&run)?;
                        return Ok(run);
                    }
                    if !evidence_sufficient {
                        run.checks.push(CheckResult {
                            criterion: Criterion {kind:CriterionKind::ToolSucceeded,path:None,contains:None,tool:Some("evidence".into())},
                            satisfied: false, explanation: "At least one real tool observation or explicit acceptance check is required".into()
                        });
                    }
                }
                self.journal.save(&run)?;
                continue;
            }
            if decision.actions.len() > 12 {
                invalid += 1;
                run.checks = vec![CheckResult {
                    criterion: Criterion {kind:CriterionKind::ToolSucceeded,path:None,
                        contains:None,tool:Some("dispatch_plan".into())},
                    satisfied:false,
                    explanation:"At most 12 tool actions may be proposed in one model step".into(),
                }];
                self.journal.save(&run)?;
                if invalid >= 3 { break; }
                continue;
            }
            // Reserve the entire proposed batch by checking the persisted
            // operation count first. Over-budget plans dispatch zero tools.
            // This also guards against slow no-progress loops with cheap reads.
            if decision.actions.len() > request.limits.max_tool_calls.saturating_sub(run.receipts.len()) {
                run.stop(Status::NeedsReview,"Tool-call budget exhausted before dispatch");
                self.journal.save(&run)?;
                return Ok(run);
            }
            // The Rust SDK, not the model, owns the dependency graph and
            // read-only concurrency. Invalid batches are sent back for
            // replanning without dispatching partial side effects.
            let groups = match scheduler::arrange(
                decision.actions, &tools, &run.receipts,
                request.limits.max_parallel_reads,
            ) {
                Ok(groups) => groups,
                Err(error) => {
                    invalid += 1;
                    run.checks = vec![CheckResult {
                        criterion: Criterion {kind:CriterionKind::ToolSucceeded,path:None,
                            contains:None,tool:Some("dispatch_plan".into())},
                        satisfied:false, explanation:error,
                    }];
                    self.journal.save(&run)?;
                    if invalid >= 3 { break; }
                    continue;
                }
            };
            invalid = 0;
            for group in groups {
                if self.journal.directive(&request.id)? != current_directive {
                    run.checks.push(CheckResult {
                        criterion: Criterion {kind:CriterionKind::ToolSucceeded,
                            path:None,contains:None,tool:Some("owner_directive".into())},
                        satisfied:false,
                        explanation:"Owner changed the task while tools were executing".into()
                    });
                    self.journal.save(&run)?;
                    continue 'steps;
                }
                if self.journal.cancelled(&request.id) {
                    run.stop(Status::Cancelled, "Owner cancelled execution");
                    self.journal.save(&run)?;
                    return Ok(run);
                }
                let mut ready = Vec::new();
                let mut skipped_any = false;
                for action in group {
                    let failed_dependency = action.depends_on.iter().find(|dependency| {
                        !run.receipts.iter().any(|receipt|
                            &receipt.id == *dependency && receipt.success)
                    });
                    if let Some(dependency) = failed_dependency {
                        skipped_any = true;
                        run.receipts.push(Receipt {
                            id:action.id.clone(),tool:action.tool.clone(),
                            arguments:action.arguments.clone(),success:false,
                            result:json!({"skipped":true,
                                "reason":format!("Dependency {dependency} was not successful")}),
                            step:run.step,
                        });
                    } else { ready.push(action); }
                }
                self.journal.save(&run)?;
                if !ready.is_empty() {
                    run.in_flight = ready.clone();
                    // Durable write-ahead marker: fail closed if storage fails.
                    self.journal.save(&run)?;
                    // A save can overlap a cancellation or instruction edit.
                    // Release only an intent that never began execution.
                    let cancelled_after_fence=self.journal.cancelled(&request.id);
                    let redirected_after_fence=self.journal.directive(&request.id)? != current_directive;
                    if cancelled_after_fence || redirected_after_fence {
                        run.in_flight.clear();
                        if cancelled_after_fence {
                            run.stop(Status::Cancelled,"Owner cancelled before tool dispatch");
                            self.journal.save(&run)?;
                            return Ok(run);
                        }
                        run.checks.push(CheckResult {
                            criterion:Criterion {kind:CriterionKind::ToolSucceeded,
                                path:None,contains:None,tool:Some("owner_directive".into())},
                            satisfied:false,
                            explanation:"Owner changed the task before tool dispatch".into(),
                        });
                        self.journal.save(&run)?;
                        continue 'steps;
                    }
                    // Kernel-local observability never reaches the host. Every
                    // external effect still passes through normal permissions.
                    let history=&run.receipts;
                    let tool_catalog=&tools;
                    let results=join_all(ready.iter().map(|action| async move {
                        let actual=match tool_discovery::resolve_bridge(action,tool_catalog,history) {
                            Ok(Some(actual))=>actual,
                            Ok(None)=>action.clone(),
                            Err(error)=>return (action.clone(),Err(error)),
                        };
                        // Every real and synthetic operation is validated against
                        // the authoritative Rust catalog before any host effect.
                        let outcome=if let Some(spec)=tool_catalog.get(&actual.tool){
                            match schema::validate_arguments(&spec.schema,&actual.arguments){
                                Err(error)=>Err(error),
                                Ok(())=>if let Some(result)=
                                    tool_discovery::execute_local(&actual,tool_catalog)
                                    .or_else(||context::execute_local(&actual,history)) {
                                    result
                                } else {self.tools.execute(&actual).await}
                            }
                        }else{Err("Tool missing from authorized Rust catalog".into())};
                        (actual,outcome)
                    })).await;
                    for (action, (actual,outcome)) in ready.iter().zip(results) {
                        let (success, result) = match outcome {
                            Ok(data) => (true, data),
                            Err(error) => (false, json!({"error":error}))
                        };
                        skipped_any |= !success;
                        run.receipts.push(Receipt {
                            id:action.id.clone(), tool:actual.tool,
                            arguments:actual.arguments,
                            success, result, step:run.step,
                        });
                    }
                    run.in_flight.clear();
                    self.journal.save(&run)?;
                }
                if matches!(request.strategy,Strategy::VerifyFirst) && !request.checks.is_empty() {
                    run.checks=self.verify(&request,&run,&tools).await;
                    self.journal.save(&run)?;
                }
                if skipped_any {
                    // Replan against observed failures; never continue with
                    // writes derived from a stale or partially failed plan.
                    run.checks.push(CheckResult {
                        criterion: Criterion {kind:CriterionKind::ToolSucceeded,path:None,
                            contains:None,tool:Some("failed_tool_batch".into())},
                        satisfied:false,
                        explanation:"A tool failed or a dependency was skipped. Inspect receipts and replan".into(),
                    });
                    self.journal.save(&run)?;
                    continue 'steps;
                }
            }
            }
        );
        run.stop(Status::NeedsReview, "Step, model or decision-repair limit reached");
        self.journal.save(&run)?;
        Ok(run)
    }

    /// Run exactly the owner's unobserved postconditions, never LLM-created
    /// test substitutes. Each call passes through the host's ordinary tool
    /// approval and durable write-ahead operation record.
    async fn run_required_commands(&self, request: &RunRequest, run: &mut Snapshot,
        tools: &HashMap<String, ToolSpec>, directive: Option<&str>,
    ) -> Result<AcceptanceRun, String> {
        for (index, check) in request.checks.iter().enumerate() {
            let Some(command) = acceptance::required_command(check) else {continue;};
            match acceptance::command_state(&run.receipts,&request.checks,tools,command) {
                acceptance::CommandState::Passed => continue,
                acceptance::CommandState::Failed => return Ok(AcceptanceRun::ExistingFailure),
                acceptance::CommandState::Pending => {}
            }
            if !tools.contains_key("shell_exec") {
                return Ok(AcceptanceRun::ToolUnavailable);
            }
            if run.receipts.len() >= request.limits.max_tool_calls {
                return Ok(AcceptanceRun::ToolBudgetExceeded);
            }
            if self.journal.cancelled(&request.id) {
                return Ok(AcceptanceRun::Cancelled);
            }
            if self.journal.directive(&request.id)?.as_deref() != directive {
                return Ok(AcceptanceRun::OwnerChanged);
            }
            let mut id = format!("{}{}-{index}",acceptance::KERNEL_VERIFICATION_PREFIX,run.step);
            while run.receipts.iter().any(|receipt|receipt.id==id) {
                id.push('x');
            }
            let action=Action {id:id.clone(),tool:"shell_exec".into(),
                arguments:json!({"command":command}),depends_on:Vec::new()};
            run.in_flight = vec![action.clone()];
            self.journal.save(run)?;
            // Owner-required commands are still effects: enforce the same
            // post-write-ahead fence as model-proposed tool operations.
            let cancelled_after_fence=self.journal.cancelled(&request.id);
            let redirected_after_fence=self.journal.directive(&request.id)?.as_deref()!=directive;
            if cancelled_after_fence || redirected_after_fence {
                run.in_flight.clear();
                self.journal.save(run)?;
                return Ok(if cancelled_after_fence {
                    AcceptanceRun::Cancelled
                } else {
                    AcceptanceRun::OwnerChanged
                });
            }
            let result=self.tools.execute(&action).await;
            let (success, result) = match result {
                Ok(output) => (true,output),
                Err(error) => (false,json!({"error":error})),
            };
            run.receipts.push(Receipt {id,tool:action.tool,
                arguments:action.arguments,success,result,step:run.step});
            run.in_flight.clear();
            // A checkpoint failure here leaves a durable unresolved
            // write-ahead marker; a restarted job cannot replay the command.
            self.journal.save(run)?;
            if !success {return Ok(AcceptanceRun::NewFailure);}
        }
        Ok(AcceptanceRun::Passed)
    }

    async fn verify(&self, request: &RunRequest, run: &Snapshot,
        tools: &HashMap<String, ToolSpec>) -> Vec<CheckResult> {
        // Independent final-byte witnesses are derived from trusted write
        // receipts and verified through the host, never from model prose.
        let artifact_witnesses=acceptance::artifact_witnesses(&run.receipts);
        if artifact_witnesses.len()>64 {
            return vec![CheckResult {
                criterion:Criterion{kind:CriterionKind::ToolSucceeded,
                    path:None,contains:None,tool:Some("artifact_witness_budget".into())},
                satisfied:false,
                explanation:format!("{} changed files exceed the 64-file final-byte verification budget; split the task or verify with an owner check",artifact_witnesses.len()),
            }];
        }
        let mut checks = Vec::with_capacity(request.checks.len()+artifact_witnesses.len());
        for criterion in request.checks.iter().chain(artifact_witnesses.iter()) {
            let observation = if criterion.kind == CriterionKind::ToolSucceeded {
                let name = criterion.tool.as_deref().unwrap_or("");
                if let Some(command)=acceptance::required_command(criterion) {
                    Ok(acceptance::command_state(&run.receipts,&request.checks,
                        tools,command)==acceptance::CommandState::Passed)
                } else {
                    Ok(run.receipts.iter().any(|r| r.tool == name && r.success))
                }
            } else {
                self.evidence.check(criterion).await
            };
            let (satisfied, explanation) = match observation {
                Ok(true) => (true, "Observed independent acceptance evidence".into()),
                Ok(false) => (false, format!("Unmet criterion: {:?}", criterion.kind)),
                Err(error) => (false, format!("Verification failed: {error}")),
            };
            checks.push(CheckResult {criterion:criterion.clone(),satisfied,explanation});
        }
        checks
    }
}

#[cfg(test)]
mod owner_effect_tests {
    use super::*;
    #[test]
    fn native_and_text_only_model_decisions_share_bounded_plans() {
        let oversized_plan=Decision{plan:vec!["step".into();33],
            actions:vec![],answer:None};
        assert!(validate_decision_plan(oversized_plan).is_err());
        let oversized_step=Decision{plan:vec!["x".repeat(1001)],
            actions:vec![],answer:None};
        assert!(validate_decision_plan(oversized_step).is_err());
        let text=serde_json::json!({"plan":vec!["step";33],
            "actions":[],"answer":null}).to_string();
        assert!(decode(&text).is_err());
        let accepted=Decision{plan:vec!["inspect".into(),"verify".into()],
            actions:vec![],answer:Some("complete".into())};
        assert!(validate_decision_plan(accepted).is_ok());
    }
    #[test]
    fn manual_uncertain_tool_observation_is_not_a_forged_success_receipt(){
        let request=RunRequest{
            id:"manual-one".into(),instruction:"write a file".into(),
            owner_instructions:String::new(),strategy:Strategy::Balanced,
            limits:Limits::default(),allowed_tools:vec!["workspace_write".into()],
            checks:vec![],require_evidence:true
        };
        let mut snapshot=Snapshot::from_request(&request);
        snapshot.status=Status::NeedsReview;
        snapshot.step=3;
        snapshot.in_flight=vec![Action{
            id:"operation-1".into(),tool:"workspace_write".into(),
            arguments:json!({"path":"result.txt","content":"written"}),
            depends_on:vec![]
        }];
        let action=snapshot.reconcile_tool_observation(true,"result.txt exists").unwrap();
        assert_eq!(action.id,"operation-1");
        assert!(matches!(snapshot.status,Status::Running));
        assert!(!snapshot.model_in_flight);
        assert!(snapshot.in_flight.is_empty());
        assert_eq!(snapshot.receipts.len(),1);
        assert_eq!(snapshot.receipts[0].success,false);
        assert_eq!(snapshot.receipts[0].result["independentlyVerified"],false);
        assert_eq!(snapshot.receipts[0].result["operatorObservedApplied"],true);
        assert!(!snapshot.checks.last().unwrap().satisfied);
        assert!(snapshot.reconcile_tool_observation(true,"again").is_err(),
            "cannot reconcile or replay an action twice");
    }
    #[test]
    fn uncertain_paid_model_and_multiple_tools_are_not_operator_resumable(){
        let request=RunRequest{
            id:"manual-two".into(),instruction:"run a shell command".into(),
            owner_instructions:String::new(),strategy:Strategy::Balanced,
            limits:Limits::default(),allowed_tools:vec![],checks:vec![],
            require_evidence:false
        };
        let mut snapshot=Snapshot::from_request(&request);
        snapshot.status=Status::NeedsReview;
        snapshot.model_in_flight=true;
        snapshot.in_flight=vec![Action{id:"x".into(),tool:"shell_exec".into(),
            arguments:json!({"command":"echo test"}),depends_on:vec![]}];
        assert!(snapshot.reconcile_tool_observation(false,"no effect").is_err());
        assert!(snapshot.in_flight.len()==1);
        snapshot.model_in_flight=false;
        snapshot.in_flight.push(snapshot.in_flight[0].clone());
        assert!(snapshot.reconcile_tool_observation(true,"could be applied").is_err());
        assert_eq!(snapshot.receipts.len(),0);
    }
    #[test]
    fn edit_intent_requires_actual_writes_in_any_supported_language() {
        for text in ["README 수정해라", "파일 작성", "코드 리팩토링", "fix the unit test", "implement tool routing"] {
            assert!(owner_requires_effect(text), "{text}");
        }
        for text in ["인사를 해줘", "현재 상황을 설명해줘", "summarize the known context"] {
            assert!(!owner_requires_effect(text), "{text}");
        }
        for text in ["README 수정해라", "코드를 구현해라", "fix the unit test"] {
            assert!(owner_requires_file_mutation(text), "{text}");
        }
        for text in ["실행해라", "배포해라", "Run cargo test", "open the browser",
            "기존 파일 읽어줘", "create a calendar event"] {
            assert!(!owner_requires_file_mutation(text), "{text}");
        }
    }
}
