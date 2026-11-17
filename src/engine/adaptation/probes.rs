//! Task-specific information-gain probes for the existing adaptation runtime.
//! No tool runs here; only the host gateway can produce real observations.
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};

const MAX_CANDIDATES: usize = 6;
const MAX_EXECUTED: usize = 2;

/// Restrict speculative tests to native observational tools. The existing
/// tool gateway still enforces user permission, cancellation and budgets.
pub fn read_only_tool(name: &str) -> bool {
    matches!(name,
        "workspace_read" | "workspace_read_range" | "workspace_list" |
        "code_search" | "memory_search" | "web_search" | "http_fetch" |
        "google_drive_search" | "google_docs_read" | "google_sheets_read" |
        "github_file_read" | "github_repository_activity" |
        "task_status" | "task_result" | "account_status")
}
pub fn max_executed() -> usize { MAX_EXECUTED }

/// A speculative observation cannot repeat the exact already-failed read
/// while the environment is unchanged.
pub fn admissible_after_failure(probe:&Value,failed_tool:&str,failed_args:&Value)->bool{
    probe["tool"].as_str().is_some_and(read_only_tool)
        && !(probe["tool"]==failed_tool && &probe["inputs"]==failed_args)
}

/// Optional extension; old adaptation documents still validate unchanged.
pub fn validate(adaptation: &Value) -> Result<(), String> {
    let Some(raw) = adaptation.get("probes") else { return Ok(()); };
    let probes = raw.as_array().ok_or("probes must be an array")?;
    if probes.len() > MAX_CANDIDATES {return Err("too many probes (max 6)".into());}
    let hypotheses = adaptation["hypotheses"].as_array()
        .ok_or("probe hypotheses must be an array")?;
    let mut ids = HashSet::new();
    for probe in probes {
        let id = probe["id"].as_str().filter(|s|!s.is_empty())
            .ok_or("probe.id must be non-empty")?;
        if !ids.insert(id) {return Err(format!("duplicate probe id {id}"));}
        let tool = probe["tool"].as_str().unwrap_or("");
        if !read_only_tool(tool) {return Err(format!("probe {id} has a non-observational tool"));}
        if !probe["inputs"].is_object(){return Err(format!("probe {id} needs object inputs"));}
        if probe["inputs"].to_string().len() > 8192 {
            return Err(format!("probe {id} inputs exceed 8KiB"));
        }
        let predictions = probe["predictions"].as_array()
            .filter(|p| p.len() >= 2 && p.len() <= 12)
            .ok_or_else(||format!("probe {id} needs 2..12 predictions"))?;
        let mut predicted = HashSet::new();
        for item in predictions {
            let hypothesis_id = item["hypothesisId"].as_str()
                .ok_or("probe prediction needs hypothesisId")?;
            if !hypotheses.iter().any(|h|h["id"]==hypothesis_id){
                return Err(format!("probe {id} predicts unknown hypothesis {hypothesis_id}"));
            }
            if !predicted.insert(hypothesis_id){
                return Err(format!("probe {id} repeats hypothesis {hypothesis_id}"));
            }
            let predicate=&item["predicate"];
            if predicate["type"]!="equals" || predicate["from"]!="result"
                || !predicate["path"].as_str().is_some_and(|p|p.starts_with('/'))
                || predicate.get("value").is_none(){
                return Err(format!("probe {id} requires equals(result, /pointer, value) predictions"));
            }
        }
        if !probe["estimatedCost"].is_null() && !probe["estimatedCost"]
            .as_f64().is_some_and(|c|c.is_finite() && c>=0.0 && c<=100.0) {
            return Err(format!("probe {id} has invalid estimatedCost"));
        }
    }
    Ok(())
}

/// Compute expected entropy reduction from distinct model-proposed output
/// predictions, never from a fixed list of tactics or the model's own rating.
/// Incomplete tests or identical outcomes have zero admissible information.
pub fn rank(adaptation: &Value, refuted: &[String], attempted: &HashSet<String>) -> Vec<Value> {
    let live = adaptation["hypotheses"].as_array().into_iter().flatten()
        .filter_map(|h|h["id"].as_str())
        .filter(|id|!refuted.iter().any(|r|r.as_str()==*id))
        .collect::<Vec<_>>();
    if live.len()<2{return Vec::new();}
    let mut ranked=Vec::new();
    for probe in adaptation["probes"].as_array().into_iter().flatten() {
        let Some(id)=probe["id"].as_str() else {continue};
        if attempted.contains(id) {continue;}
        let Some(tool)=probe["tool"].as_str() else {continue};
        if !read_only_tool(tool) {continue;}
        let Some(predictions)=probe["predictions"].as_array() else {continue};
        let mut buckets = BTreeMap::<String,usize>::new();
        let mut common_path=None;
        let mut valid=true;
        for hypothesis_id in &live {
            let Some(item)=predictions.iter().find(|p|p["hypothesisId"].as_str()==Some(hypothesis_id)) else {
                valid=false;break;
            };
            let predicate=&item["predicate"];
            let Some(path)=predicate["path"].as_str() else {valid=false;break};
            if common_path.is_some_and(|prior|prior!=path){valid=false;break;}
            common_path=Some(path);
            if predicate["type"]!="equals" || predicate["from"]!="result" {valid=false;break;}
            let value=predicate["value"].to_string();
            *buckets.entry(value).or_default()+=1;
        }
        if !valid || buckets.len()<2 {continue;}
        let total=live.len() as f64;
        let information_bits=buckets.values().map(|n|{
            let p=*n as f64/total;
            -p*p.log2()
        }).sum::<f64>();
        let estimated_cost=probe["estimatedCost"].as_f64().unwrap_or(0.5);
        let score=information_bits/(1.0+estimated_cost);
        let mut choice=probe.clone();
        choice["expectedInformationBits"]=json!(information_bits);
        choice["informationPerCost"]=json!(score);
        ranked.push(choice);
    }
    ranked.sort_by(|a,b|{
        b["informationPerCost"].as_f64().unwrap_or(0.0)
            .total_cmp(&a["informationPerCost"].as_f64().unwrap_or(0.0))
            .then_with(||a["id"].as_str().cmp(&b["id"].as_str()))
    });
    // Same tool/inputs cannot buy extra information, even when the model
    // attaches different claims. Keep the *best* ranked proposal after sort.
    let mut seen_calls=HashSet::new();
    ranked.retain(|probe|seen_calls.insert(format!(
        "{}:{}",probe["tool"].as_str().unwrap_or(""),probe["inputs"]
    )));
    ranked
}

/// Update hypotheses from actual successful tool output via the existing
/// three-valued predicate evaluator. Unknown never refutes a hypothesis.
pub fn observe(probe: &Value, result: &Value) -> Value {
    let scope=json!({"result":result});
    let mut observed=Vec::new();
    let mut refuted=Vec::new();
    for prediction in probe["predictions"].as_array().into_iter().flatten() {
        let id=prediction["hypothesisId"].as_str().unwrap_or("");
        let verdict=crate::engine::program::predicate_verdict(&prediction["predicate"],&scope)
            .map(|v|v.as_str().to_string()).unwrap_or_else(|_|"unknown".into());
        if verdict=="false"{refuted.push(id.to_string());}
        observed.push(json!({"hypothesisId":id,"verdict":verdict}));
    }
    json!({"probeId":probe["id"],"tool":probe["tool"],
        "predictions":observed,"refutedIds":refuted})
}

#[cfg(test)]
mod tests {
    use super::*;
    fn plan()->Value {
        json!({"hypotheses":[{"id":"h1"},{"id":"h2"},{"id":"h3"}],
            "probes":[
                {"id":"uninformative","tool":"workspace_read","inputs":{"path":"a"},
                    "predictions":[
                        {"hypothesisId":"h1","predicate":{"type":"equals","from":"result","path":"/kind","value":"same"}},
                        {"hypothesisId":"h2","predicate":{"type":"equals","from":"result","path":"/kind","value":"same"}},
                        {"hypothesisId":"h3","predicate":{"type":"equals","from":"result","path":"/kind","value":"same"}}]},
                {"id":"discriminate","tool":"workspace_list","inputs":{"path":"src"},"estimatedCost":0.1,
                    "predictions":[
                        {"hypothesisId":"h1","predicate":{"type":"equals","from":"result","path":"/kind","value":"absent"}},
                        {"hypothesisId":"h2","predicate":{"type":"equals","from":"result","path":"/kind","value":"present"}},
                        {"hypothesisId":"h3","predicate":{"type":"equals","from":"result","path":"/kind","value":"corrupt"}}]}
            ]})
    }
    #[test]
    fn ranks_derived_predictions_not_fixed_tactics(){
        let p=plan();
        assert!(validate(&p).is_ok());
        let ranked=rank(&p,&[],&HashSet::new());
        assert_eq!(ranked.len(),1);
        assert_eq!(ranked[0]["id"],"discriminate");
        assert!(ranked[0]["expectedInformationBits"].as_f64().unwrap()>1.5);
        assert!(rank(&p,&[],&HashSet::from(["discriminate".to_owned()])).is_empty());
    }
    #[test]
    fn actual_result_refutes_alternatives_without_calling_model(){
        let p=plan();let probe=&p["probes"][1];
        let observation=observe(probe,&json!({"kind":"present"}));
        assert_eq!(observation["refutedIds"],json!(["h1","h3"]));
        let missing=observe(probe,&json!({"other":3}));
        assert_eq!(missing["refutedIds"],json!([]));
        assert_eq!(missing["predictions"][0]["verdict"],"unknown");
        assert!(rank(&p,&["h1".into(),"h3".into()],&HashSet::new()).is_empty());
    }
    #[test]
    fn rejects_mutating_and_unverifiable_probes(){
        let mut p=plan();
        p["probes"][0]["tool"]=json!("shell_exec");
        assert!(validate(&p).is_err());
        p["probes"][0]["tool"]=json!("workspace_read");
        p["probes"][0]["predictions"][0]["predicate"]["from"]=json!("model_claim");
        assert!(validate(&p).is_err());
        assert!(!read_only_tool("github_api"));
        assert!(!read_only_tool("workspace_write"));
        let probe=json!({"tool":"workspace_read","inputs":{"path":"config.txt"}});
        assert!(!admissible_after_failure(&probe,"workspace_read",&json!({"path":"config.txt"})));
        assert!(admissible_after_failure(&probe,"workspace_read",&json!({"path":"other.txt"})));
    }
    #[test]
    fn duplicate_probe_inputs_keep_only_best_information_per_cost(){
        let mut p=plan();
        let mut weaker=p["probes"][1].clone();
        weaker["id"]=json!("duplicate");
        weaker["estimatedCost"]=json!(9.0);
        p["probes"].as_array_mut().unwrap().insert(0,weaker);
        let ranked=rank(&p,&[],&HashSet::new());
        assert_eq!(ranked.len(),1);
        assert_eq!(ranked[0]["id"],"discriminate");
    }
    #[test]
    fn incomplete_predictions_cannot_earn_information_gain(){
        let mut p=plan();
        p["probes"][1]["predictions"].as_array_mut().unwrap().pop();
        assert!(rank(&p,&[],&HashSet::new()).is_empty());
    }
}
