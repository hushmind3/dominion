//! One native admission path for speculative read-only observations.
//! Reused by failure hypothesis tests, ProgramIR candidate preflights and
//! post-execution verification. The regular dispatcher retains all actual
//! tool authorization, effect sealing, metrics, cancellation and receipts.
use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) async fn dispatch_observation(
    ctx: &ProgramCtx<'_>,
    tool: &str,
    inputs: Value,
    origin: &str,
    round: usize,
    budget: &mut super::budget::BudgetLedger,
    op_seq: &mut u64,
) -> Result<Option<(Value, bool, String)>, String> {
    if !crate::engine::adaptation::probes::read_only_tool(tool) {
        return Err(format!("non-observational probe tool denied: {tool}"));
    }
    if !inputs.is_object() || inputs.to_string().len() > 8192 {
        return Err("probe inputs must be a bounded object".into());
    }
    if ctx.state.is_cancelled(ctx.session_id)
        || ctx.state.cancellation_generation(ctx.session_id) != ctx.generation {
        return Err("실행이 중단되었습니다.".into());
    }
    if !budget.charge_tool() {
        return Ok(None);
    }
    *op_seq += 1;
    let call=ToolCall{
        id:format!("native-observation-{}",uuid::Uuid::new_v4()),
        name:tool.to_string(),arguments:inputs.to_string(),
    };
    let (result,accepted,_)=dispatch_tool_call(
        ctx.state,ctx.tx,ctx.session,ctx.request,
        ctx.session_id,ctx.execution_id,&call,inputs,ctx.metrics,
        *op_seq,round,0,false,origin,None,None,None,
    ).await?;
    let observed_success=accepted && tool_succeeded(&result);
    Ok(Some((result,observed_success,call.id)))
}
