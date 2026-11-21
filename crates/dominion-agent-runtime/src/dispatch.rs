//! Durable, dependency-aware tool execution lane shared by native chat and
//! structured model planners. The host owns permissions and read classification.
//! Completed receipts are the recovery authority; no side effects are replayed.
use crate::{Action, Dispatcher, Journal, Receipt, Snapshot, Status};
use futures_util::future::join_all;
use serde_json::json;
use std::collections::HashSet;

pub async fn dispatch_batch<D: Dispatcher>(
    run: &mut Snapshot, actions: &[Action], dispatcher: &D, journal: &dyn Journal,
) -> Result<Vec<Result<D::Output, String>>, String> {
    if !matches!(run.status, Status::Running) {
        return Err("종료된 실행에 도구를 추가할 수 없습니다.".into());
    }
    if journal.cancelled(&run.id) {
        return Err("실행이 취소되었습니다.".into());
    }
    if !run.in_flight.is_empty() {
        return Err("이전 도구 실행의 결과를 먼저 확인해야 합니다.".into());
    }
    // A model that may still be in flight has not produced a committed
    // decision yet. A caller must settle that paid observation before the
    // shared tool lane can perform even a read, much less a side effect.
    if run.model_in_flight {
        return Err("모델 응답의 영속 기록을 완료하기 전에는 도구를 실행할 수 없습니다.".into());
    }
    // A new owner directive invalidates the entire pending batch.
    // Recheck between checkpoints: earlier tool receipts remain durable,
    // but no later side effect may execute under the abandoned instruction.
    let planned_directive = journal.directive(&run.id)?;
    let mut ids = HashSet::new();
    for action in actions {
        if action.id.trim().is_empty() || action.id.len() > 128
            || !ids.insert(action.id.clone())
            || run.receipts.iter().any(|receipt| receipt.id == action.id) {
            return Err("중복되거나 비어 있는 도구 실행 ID입니다.".into());
        }
        // An action may depend on a previous successful receipt OR an
        // earlier action in this batch. Forward dependencies are rejected.
        for dependency in &action.depends_on {
            if dependency == &action.id || (!ids.contains(dependency)
                && !run.receipts.iter().any(|receipt|
                    &receipt.id == dependency && receipt.success)) {
                return Err(format!("도구 실행의 선행 조건이 충족되지 않았습니다: {}", action.id));
            }
        }
    }
    let mut outputs = Vec::with_capacity(actions.len());
    let mut cursor = 0usize;
    while cursor < actions.len() {
        if journal.cancelled(&run.id) {
            return Err("실행이 취소되었습니다.".into());
        }
        if journal.directive(&run.id)? != planned_directive {
            return Err("작업 지시가 변경되어 기존 도구 계획을 중단했습니다.".into());
        }
        let parallel_end = crate::read_batch_end(
            actions, cursor, dispatcher.max_parallel_reads().clamp(1, 16),
            |action| dispatcher.is_read_only(action),
        );
        // A dependent read cannot start in parallel with the producer.
        let mut end = cursor + 1;
        while end < parallel_end && !actions[end].depends_on.iter().any(
            |dependency| actions[cursor..end].iter().any(|earlier|&earlier.id == dependency)
        ) { end += 1; }
        let group = &actions[cursor..end];
        if group.iter().any(|action| action.depends_on.iter().any(|dependency|
            !run.receipts.iter().any(|receipt|
                &receipt.id == dependency && receipt.success))) {
            outputs.extend(actions[cursor..].iter().map(|action|
                Err(format!("Skipped {} because a required tool did not succeed", action.id))));
            break;
        }
        run.in_flight = group.to_vec();
        // Persist intent before starting any tool. A failed save dispatches nothing.
        journal.save(run)?;
        // An instruction may change between planning and the durable fence.
        // No tool has run yet, so release the fence and stop cleanly.
        let cancelled = journal.cancelled(&run.id);
        let redirected = journal.directive(&run.id)? != planned_directive;
        if cancelled || redirected {
            run.in_flight.clear();
            journal.save(run)?;
            return Err(if cancelled {"실행이 취소되었습니다."} else {
                "작업 지시가 변경되어 기존 도구 계획을 중단했습니다."
            }.into());
        }
        let results = join_all(group.iter().map(|action| dispatcher.execute(action))).await;
        let mut group_failed = false;
        for (action, outcome) in group.iter().zip(results) {
            let (success, observation, output) = match outcome {
                Ok(result) if result.success => (true, result.observation, Ok(result.output)),
                // A completed tool can report failure without a transport error.
                // Preserve its host result so the model sees the observation and
                // can repair the task. The durable receipt remains unsuccessful.
                Ok(result) => (false, result.observation, Ok(result.output)),
                Err(error) => (false, json!({"error":error.clone()}), Err(error)),
            };
            group_failed |= !success;
            run.receipts.push(Receipt { id: action.id.clone(), tool: action.tool.clone(),
                arguments: action.arguments.clone(), success, result: observation, step: run.step });
            outputs.push(output);
        }
        run.in_flight.clear();
        // The next group may not run if completed receipts cannot be persisted.
        journal.save(run)?;
        cursor = end;
        if group_failed {
            // Return an output slot for each skipped action without forging a
            // receipt or reserving an execution that never occurred.
            outputs.extend(actions[cursor..].iter().map(|action|
                Err(format!("Skipped {} after an unsuccessful tool batch", action.id))));
            break;
        }
    }
    Ok(outputs)
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize,Ordering};

    struct NoEffect(AtomicUsize);
    impl Dispatcher for NoEffect {
        type Output = ();
        fn execute<'a>(&'a self,_action:&'a Action)
            -> crate::BoxFuture<'a,Result<crate::DispatchResult<Self::Output>,String>> {
            self.0.fetch_add(1,Ordering::SeqCst);
            Box::pin(async {Ok(crate::DispatchResult {
                output:(),observation:json!({"ok":true}),success:true
            })})
        }
    }
    struct InMemoryJournal;
    impl Journal for InMemoryJournal {
        fn save(&self,_:&Snapshot)->Result<(),String>{Ok(())}
    }
    fn ready<T>(f:impl std::future::Future<Output=T>)->T {
        use std::future::Future;
        let waker=std::task::Waker::noop();
        let mut context=std::task::Context::from_waker(waker);
        let mut pending=Box::pin(f);
        match Future::poll(pending.as_mut(),&mut context) {
            std::task::Poll::Ready(value)=>value,
            std::task::Poll::Pending=>panic!("unexpected pending dispatch")
        }
    }

    #[test]
    fn model_in_flight_blocks_tool_dispatch_before_any_host_effect() {
        let req=crate::RunRequest {
            id:"run-guard".into(),instruction:"read a file".into(),
            owner_instructions:String::new(),strategy:crate::Strategy::Balanced,
            limits:crate::Limits::default(),allowed_tools:vec!["read".into()],
            checks:Vec::new(),require_evidence:false
        };
        let mut run=Snapshot::from_request(&req);
        run.model_in_flight=true;
        let dispatcher=NoEffect(AtomicUsize::new(0));
        let actions=[Action {
            id:"read-1".into(),tool:"read".into(),
            arguments:json!({}),depends_on:Vec::new()
        }];
        let outcome=ready(dispatch_batch(&mut run,&actions,&dispatcher,&InMemoryJournal));
        assert!(outcome.is_err());
        assert!(run.in_flight.is_empty());
        assert!(run.receipts.is_empty());
        assert!(run.model_in_flight);
        assert_eq!(dispatcher.0.load(Ordering::SeqCst),0);
    }
}
