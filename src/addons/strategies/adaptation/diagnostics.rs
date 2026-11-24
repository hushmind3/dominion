//! Host adapter for diagnostics. The actual native observation-only gate
//! stays authoritative; the algorithm cannot execute its selected probe.
use serde_json::Value;
pub use dominion_algorithms::diagnostics::{signature,observe,contradicted};

pub fn choose_with_budget(
    adaptation:&Value,refuted:&[String],attempted:&[String],failed_tool:&str,
    failed_inputs:&Value,remaining_steps:usize,
)->Option<Value>{
    dominion_algorithms::diagnostics::choose_with_budget(adaptation,refuted,attempted,
        failed_tool,failed_inputs,remaining_steps,crate::engine::effect::is_observation_only)
}

/// Test-only compatibility: production always uses remaining live budget.
#[cfg(test)]
pub fn choose(
    adaptation: &Value,
    refuted: &[String],
    attempted: &[String],
    failed_tool: &str,
    failed_inputs: &Value,
) -> Option<Value> {
    choose_with_budget(adaptation, refuted, attempted, failed_tool, failed_inputs, 3)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn candidates() -> Value {
        json!({
            "hypotheses":[{"id":"file_missing"},{"id":"permission"},{"id":"stale_cache"}],
            "diagnostics":[
                {"id":"not_informative","tool":"workspace_list","inputs":{"path":"."},
                 "predicate":{"type":"exists","from":"result","path":"/entries"},
                 "predictions":[
                   {"hypothesisId":"file_missing","expected":true},
                   {"hypothesisId":"permission","expected":true},
                   {"hypothesisId":"stale_cache","expected":true}]},
                {"id":"path_probe","tool":"workspace_read","inputs":{"path":"src/main.rs"},
                 "predicate":{"type":"exists","from":"result","path":"/content"},
                 "estimatedCost":0.2,
                 "predictions":[
                   {"hypothesisId":"file_missing","expected":false},
                   {"hypothesisId":"permission","expected":false},
                   {"hypothesisId":"stale_cache","expected":true}]},
                {"id":"unsafe","tool":"shell_exec","inputs":{"command":"rm -rf tmp"},
                 "predicate":{"type":"exists","from":"result","path":"/content"},
                 "predictions":[
                   {"hypothesisId":"file_missing","expected":true},
                   {"hypothesisId":"permission","expected":false}]}
            ]
        })
    }

    #[test]
    fn only_a_discriminating_independent_read_is_eligible() {
        let selected = choose(&candidates(), &[], &[], "workspace_write", &json!({})).unwrap();
        assert_eq!(selected["id"], "path_probe");
        assert_eq!(selected["forecastWorstRemaining"], 2);
        assert!(selected.get("informationGain").is_none());
        assert!(selected.get("selectionScore").is_none());
    }

    #[test]
    fn repeated_experiments_and_mutating_tools_are_not_new_information() {
        let all = candidates();
        let used = vec![signature("workspace_read", &json!({"path":"src/main.rs"}))];
        assert!(choose(&all, &[], &used, "workspace_write", &json!({})).is_none());
        assert!(choose(&all, &[], &[], "workspace_read",
            &json!({"path":"src/main.rs"})).is_none());
        assert!(choose(&all, &["file_missing".into(), "permission".into()],
            &[], "workspace_write", &json!({})).is_none());
    }

    #[test]
    fn contradictory_real_observation_updates_hypothesis_set() {
        let item = choose(&candidates(), &[], &[], "workspace_write", &json!({})).unwrap();
        let evidence = observe(&item, &json!({"content":"real bytes"}), true, "op-101");
        assert_eq!(evidence["verdict"], "true");
        assert_eq!(evidence["supports"], json!(["stale_cache"]));
        assert_eq!(contradicted(&[evidence]), vec!["file_missing", "permission"]);
    }

    #[test]
    fn failed_or_unknown_read_cannot_prove_a_cause() {
        let item = choose(&candidates(), &[], &[], "workspace_write", &json!({})).unwrap();
        for (result, ok) in [(json!({"error":"denied"}),false), (json!({}),true)] {
            let evidence = observe(&item, &result, ok, "op-5");
            assert!(contradicted(&[evidence]).is_empty());
        }
    }

    #[test]
    fn prediction_fabrication_does_not_bypass_tool_allowlist() {
        let mut data = candidates();
        data["diagnostics"][1]["tool"] = json!("workspace_write");
        assert!(choose(&data,&[],&[],"a",&json!({})).is_none());
        data["diagnostics"][1]["tool"] = json!("workspace_read");
        data["diagnostics"][1]["inputs"] = json!({"path":{"from":"unverified"}});
        assert!(choose(&data,&[],&[],"a",&json!({})).is_none());
        data["diagnostics"][1]["inputs"] = json!({"path":"../.ssh/id_rsa"});
        assert!(choose(&data,&[],&[],"a",&json!({})).is_none());
        data["diagnostics"][1]["inputs"] = json!({"path":"/etc/passwd"});
        assert!(choose(&data,&[],&[],"a",&json!({})).is_none());
    }
    #[test]
    fn contingent_two_step_policy_beats_greedy_first_split() {
        // Nine distinct causes; greedy one-step picks probe_6 (5/4),
        // leaving at least four possible causes even after one more probe.
        // A contingent first choice probe_1 (3/6) can reduce both branches
        // to <=3 with a second independent observation.
        let masks = [508u32, 292, 501, 498, 34, 380, 472];
        let hypotheses = (0..9).map(|i| json!({"id": format!("h{i}")}))
            .collect::<Vec<_>>();
        let diagnostics = masks.iter().enumerate().map(|(i,mask)| json!({
            "id":format!("probe_{i}"),"tool":"workspace_read",
            "inputs":{"path":format!("fixture/probe_{i}.txt")},
            "predicate":{"type":"equals","from":"result","path":"/flag","value":true},
            "predictions": (0..9).map(|id| json!({
                "hypothesisId":format!("h{id}"),"expected": (mask & (1u32 << id)) != 0
            })).collect::<Vec<_>>()
        })).collect::<Vec<_>>();
        let scenario = json!({"hypotheses":hypotheses,"diagnostics":diagnostics});
        let first = choose_with_budget(&scenario,&[],&[],"workspace_write",&json!({}),2).unwrap();
        assert_eq!(first["id"],"probe_1");
        assert_eq!(first["forecastWorstRemaining"],3);
        assert_eq!(first["selectionPolicy"],"contingent_minimax_v1");
        let greedy = choose_with_budget(&scenario,&[],&[],"workspace_write",&json!({}),1).unwrap();
        assert_eq!(greedy["id"],"probe_6");
        assert_eq!(greedy["forecastWorstRemaining"],5);
        for flag in [false,true] {
            let receipt = observe(&first,&json!({"flag":flag}),true,"op-proof");
            let refuted = contradicted(&[receipt]);
            let used = vec![signature(first["tool"].as_str().unwrap(),&first["inputs"])];
            let follow = choose_with_budget(
                &scenario,&refuted,&used,"workspace_write",&json!({}),1
            ).unwrap();
            assert_ne!(follow["id"],first["id"]);
            assert!(follow["forecastWorstRemaining"].as_u64().unwrap() <= 3);
        }
    }

    #[test]
    fn incomplete_or_conflicting_diagnostic_predictions_are_rejected() {
        let mut data = candidates();
        data["diagnostics"][1]["predictions"].as_array_mut().unwrap().pop();
        assert!(choose(&data,&[],&[],"workspace_write",&json!({})).is_none());
        let mut data = candidates();
        data["diagnostics"][1]["predictions"].as_array_mut().unwrap()
            .push(json!({"hypothesisId":"stale_cache","expected":false}));
        assert!(choose(&data,&[],&[],"workspace_write",&json!({})).is_none());
    }

    #[test]
    fn diagnostic_horizon_obeys_the_remaining_execution_budget() {
        assert!(choose_with_budget(&candidates(),&[],&[],"x",&json!({}),0).is_none());
        let one = choose_with_budget(&candidates(),&[],&[],"x",&json!({}),1).unwrap();
        assert_eq!(one["forecastHorizon"],1);
        let many = choose_with_budget(&candidates(),&[],&[],"x",&json!({}),100).unwrap();
        assert_eq!(many["forecastHorizon"],1);
    }

    #[test]
    fn observed_cause_changes_the_selected_remediation() {
        let data = json!({
            "hypotheses":[{"id":"missing"},{"id":"permission"},{"id":"stale"}],
            "diagnostics":[{
                "id":"read","tool":"workspace_read","inputs":{"path":"source.rs"},
                "predicate":{"type":"equals","from":"result","path":"/reachable","value":true},
                "predictions":[
                    {"hypothesisId":"missing","expected":false},
                    {"hypothesisId":"permission","expected":false},
                    {"hypothesisId":"stale","expected":true}]
            }],
            "interventions":[
                {"id":"create","tool":"workspace_write","inputs":{"path":"source.rs","content":"a"},
                    "hypothesisIds":["missing"],"expectedProgress":2.0},
                {"id":"refresh","tool":"workspace_read","inputs":{"path":"actual.rs"},
                    "hypothesisIds":["stale"],"expectedProgress":1.0}
            ]
        });
        let before = crate::engine::adaptation_loop::choose_intervention_with_context(
            &data,"shell_exec",&json!({"command":"failed"}),false,&json!({}),&[]
        ).unwrap();
        assert_eq!(before["id"],"create");
        let diagnostic = choose(&data,&[],&[],"shell_exec",&json!({})).unwrap();
        let receipt = observe(&diagnostic,&json!({"reachable":true}),true,"op-11");
        let after = crate::engine::adaptation_loop::choose_intervention_with_context(
            &data,"shell_exec",&json!({"command":"failed"}),false,&json!({}),
            &contradicted(&[receipt])
        ).unwrap();
        assert_eq!(after["id"],"refresh");
    }

}
