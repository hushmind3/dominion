//! Separate agent iteration algorithm, composed by the product gateway.
//! All effect, cancellation, budget and journal authority stays in core.
pub use dominion_core::runtime_port::{Context,Flow,Host,ModelDecision,model_call};
use dominion_core::domain::RunStatus;
#[cfg(test)]
use serde_json::{json,Value};
#[cfg(test)]
use dominion_core::{
    budget::BudgetLedger,
    hosted::{BoxFuture,Journal,Snapshot},
};

pub struct Engine;
impl Engine {
    /// This is the sole agent loop for every product entry point.
    pub async fn drive<H: Host>(host: &mut H, mut ctx: Context<'_>) -> Result<(), String> {
        ctx.check_cancelled()?;
        if ctx.snapshot.native.status != RunStatus::Running {
            return Err("실행 중인 네이티브 프레임만 진행할 수 있습니다.".into());
        }
        if !ctx.snapshot.settled() {
            return Err(
                "이전 실행·승인·하위 작업의 결과를 확인하기 전에는 초기화나 모델 호출을 재개할 수 없습니다.".into(),
            );
        }
        ctx.journal.save(&ctx.snapshot)?;
        ctx.phase("initialize")?;
        let mut complete = host.initialize(&mut ctx).await? == Flow::Complete;
        if complete {
            complete = host.confirm_completion()? == Flow::Complete;
        }
        if !complete && host.wants_plan() {
            ctx.snapshot = host.reload()?;
            ctx.check_cancelled()?;
            ctx.phase("planning")?;
            ctx.snapshot.begin_model(ctx.journal)?;
            let plan = host.plan(&mut ctx).await;
            if let Ok(Some(plan)) = &plan {
                ctx.snapshot.native.plan = Some(plan.clone());
            }
            ctx.snapshot.finish_model(ctx.journal)?;
            plan?;
        }
        while !complete {
            ctx.snapshot = host.reload()?;
            ctx.check_cancelled()?;
            // A completed tool round may have met all frozen postconditions.
            // Reconcile before reserving the next model call, including when
            // the last paid round was used to perform the successful mutation.
            // In-flight tool/model intents can never be bypassed here.
            if ctx.snapshot.settled()
                && host.reconcile_verified(&mut ctx).await? == Flow::Complete
            {
                if host.confirm_completion()? == Flow::Complete {
                    ctx.phase("verified_checkpoint")?;
                    break;
                }
            }
            ctx.snapshot = host.reload()?;
            ctx.check_cancelled()?;
            let Some(round) = ctx.snapshot.next_round(&mut ctx.budget, ctx.journal)? else {
                host.exhausted()?;
                if host.confirm_completion()? != Flow::Complete {
                    return Err("예산 소진 후 작업 지시가 변경되어 완료할 수 없습니다.".into());
                }
                break;
            };
            ctx.round = round;
            ctx.phase("prepare_model")?;
            let catalog = host.prepare_model(&mut ctx).await?;
            ctx.snapshot = host.reload()?;
            // Preparation can await owner directives or context work. Recheck
            // the live deadline and state before writing a phase or paid intent.
            ctx.check_cancelled()?;
            if ctx.snapshot.native.status != RunStatus::Running {
                return Err("실행 중인 작업만 모델을 호출할 수 있습니다.".into());
            }
            ctx.snapshot.native.advertised_tools = catalog;
            ctx.phase("model")?;
            ctx.snapshot.begin_model(ctx.journal)?;
            let reply = host.model(&mut ctx).await;
            ctx.snapshot.finish_model(ctx.journal)?;
            let reply = reply?;
            let decision = host.observe_model(&mut ctx, reply).await?;
            ctx.snapshot = host.reload()?;
            match decision {
                ModelDecision::Continue => ctx.phase("superseded")?,
                ModelDecision::Verify => {
                    ctx.phase("verify")?;
                    complete = host.verify(&mut ctx).await? == Flow::Complete;
                }
                ModelDecision::Tools => {
                    ctx.phase("tools")?;
                    let count = host.begin_tools(&mut ctx).await?;
                    for index in 0..count {
                        ctx.check_cancelled()?;
                        if !host.prepare_tool(&mut ctx, index).await? {
                            continue;
                        }
                        let result = host.dispatch_tool(&mut ctx, index).await?;
                        host.observe_tool(&mut ctx, index, result).await?;
                        ctx.snapshot = host.reload()?;
                    }
                    ctx.phase("verify_tools")?;
                    complete = host.verify_tools(&mut ctx).await? == Flow::Complete;
                }
            }
            if complete {
                complete = host.confirm_completion()? == Flow::Complete;
            }
        }
        ctx.snapshot = host.reload()?;
        let (answer, calls, tokens) = host.completion()?;
        ctx.snapshot.model_calls = ctx.snapshot.model_calls.max(calls);
        ctx.snapshot.charged_tokens = tokens;
        ctx.phase("complete")?;
        ctx.snapshot.complete(answer, ctx.journal)
    }
}

/// The default ReAct-style agent loop implements the native interface.
impl<H: Host> dominion_core::runtime_port::AgentLoop<H> for Engine {
    fn run<'a>(
        &'a self,
        host: &'a mut H,
        ctx: Context<'a>,
    ) -> dominion_core::hosted::BoxFuture<'a, Result<(), String>> {
        Box::pin(Engine::drive(host, ctx))
    }
}

#[cfg(test)]
#[path = "engine_tests.rs"]
mod tests;
