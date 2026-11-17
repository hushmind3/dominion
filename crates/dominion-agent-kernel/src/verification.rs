//! Model-independent chronological evidence verification.
//! The host normalizes durable tool receipts. This kernel never runs tools.
#[derive(Debug,Clone,PartialEq,Eq)]
pub enum EvidenceEvent {
    FileMutation { observed: bool },
    Command { command: String, success: bool, independent_test: bool, can_mutate: bool },
}

/// A historical verifier pass expires after a later attempted write,
/// failed retest, or another potentially mutating command.
pub fn command_is_fresh(events:&[EvidenceEvent],required:&str)->bool{
    let required=required.trim();
    if required.is_empty(){return false;}
    let mut current=false;
    for event in events{
        match event{
            EvidenceEvent::FileMutation{..}=>current=false,
            EvidenceEvent::Command{command,success,can_mutate,..}=>{
                if command.trim()==required{current=*success;}
                else if *can_mutate{current=false;}
            }
        }
    }
    current
}
/// An inferred code-edit completion needs a successful test of the final
/// attempted workspace state, with at least one observed file edit.
pub fn has_post_edit_test(events:&[EvidenceEvent])->bool{
    let mut edited=false;
    let mut current=false;
    for event in events{
        match event{
            EvidenceEvent::FileMutation{observed}=>{
                edited |= observed;
                current=false;
            }
            EvidenceEvent::Command{success,independent_test,can_mutate,..}=>{
                if *independent_test {current=edited && *success;}
                else if *can_mutate {current=false;}
            }
        }
    }
    edited&&current
}
#[cfg(test)]
mod tests{
    use super::*;
    fn edit(observed:bool)->EvidenceEvent{EvidenceEvent::FileMutation{observed}}
    fn test(success:bool)->EvidenceEvent{EvidenceEvent::Command{
        command:"cargo test".into(),success,independent_test:true,can_mutate:false
    }}
    #[test]fn failed_partial_write_invalidates_last_test(){
        let mut tape=vec![edit(true),test(true)];
        assert!(command_is_fresh(&tape,"cargo test"));
        assert!(has_post_edit_test(&tape));
        tape.push(edit(false));
        assert!(!command_is_fresh(&tape,"cargo test"));
        assert!(!has_post_edit_test(&tape));
        tape.push(test(true));
        assert!(command_is_fresh(&tape,"cargo test"));
        assert!(has_post_edit_test(&tape));
    }
    #[test]fn failed_retest_overrides_previous_success(){
        let tape=vec![edit(true),test(true),test(false)];
        assert!(!command_is_fresh(&tape,"cargo test"));
        assert!(!has_post_edit_test(&tape));
        assert!(command_is_fresh(&[test(true)],"cargo test"));
        assert!(!has_post_edit_test(&[test(true)]));
    }
    #[test]fn harmless_read_keeps_pass_and_opaque_command_revokes_it(){
        let mut tape=vec![edit(true),test(true)];
        tape.push(EvidenceEvent::Command{command:"git status".into(),
            success:true,independent_test:false,can_mutate:false});
        assert!(command_is_fresh(&tape,"cargo test"));
        assert!(has_post_edit_test(&tape));
        tape.push(EvidenceEvent::Command{command:"git checkout main".into(),
            success:true,independent_test:false,can_mutate:true});
        assert!(!command_is_fresh(&tape,"cargo test"));
        assert!(!has_post_edit_test(&tape));
    }
    #[test]fn explicit_custom_verifier_does_not_fake_generic_code_test(){
        let custom=EvidenceEvent::Command{command:"./verify-task.sh".into(),
            success:true,independent_test:false,can_mutate:true};
        assert!(command_is_fresh(&[custom.clone()],"./verify-task.sh"));
        assert!(!has_post_edit_test(&[edit(true),custom]));
    }
}
