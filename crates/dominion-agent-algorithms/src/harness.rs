//! The single executable turn iteration owned by the Algorithm Harness.
//! All model types use this same loop source. Runtime still enforces
//! non-overridable budgets, authority, model admission and receipts.
/// The only step-loop implementation for native ReAct and streamed Chat.
/// Adapters supply their policy-specific turn body and cancellation settlement,
/// while admission ordering and the execution clock stay in one Rust source.
#[macro_export]
macro_rules! harness_steps {
    (
        $label:lifetime : $drive:ident,
        running = $running:expr,
        cancelled = $cancelled:expr,
        calls = $calls:expr,
        limit = $limit:expr,
        on_cancel = $on_cancel:block,
        |$step:ident| $body:block
    ) => {
        $label: loop {
            if !($running) { break $label; }
            let $step=match $drive.admit($cancelled,$calls,$limit){
                $crate::runtime::DriveAdmission::Next(next)=>next,
                $crate::runtime::DriveAdmission::Cancelled=>$on_cancel,
                $crate::runtime::DriveAdmission::Exhausted=>break $label,
            };
            $body
        }
    };
}

#[cfg(test)]
mod tests{
    use crate::runtime;
    #[test]fn both_loop_styles_share_one_control_flow_source(){
        let mut drive=runtime::HarnessDrive::new(8).unwrap();
        let mut seen=Vec::new();
        let mut running=true;
        crate::harness_steps!('steps:drive,running=running,
            cancelled=false,calls=seen.len(),limit=8,
            on_cancel={panic!("should not cancel")},
            |step|{
                seen.push(step);
                if step==0{continue 'steps;}
                if step==2{running=false;}
            }
        );
        assert_eq!(seen,vec![0,1,2]);
        assert_eq!(drive.completed_steps(),3);
    }

    #[test]
    fn stopped_adapter_never_admits_or_executes_a_step(){
        let mut drive=runtime::HarnessDrive::new(8).unwrap();
        let running=false;
        let mut cancellation_checks=0;
        let mut executed=0;
        crate::harness_steps!('steps:drive,running=running,
            cancelled={cancellation_checks+=1;false},calls=0,limit=8,
            on_cancel={break 'steps},
            |_step|{executed+=1;}
        );
        assert_eq!(drive.completed_steps(),0);
        assert_eq!(cancellation_checks,0);
        assert_eq!(executed,0);
    }

}
