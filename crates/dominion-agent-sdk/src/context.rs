//! Durable, budgeted context projection for the independently owned Rust loop.
//!
//! Informed by Codex history compaction and DeepSeek's event-sourced
//! tool-result projection: model-visible context may be pruned, but the
//! authoritative receipts never are. Old results can be searched/read
//! through safe, kernel-local, read-only tools without external permissions.
use crate::{Action, Receipt, Strategy, ToolSpec};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) const RECEIPT_READ: &str = "dominion_receipt_read";
pub(crate) const RECEIPT_SEARCH: &str = "dominion_receipt_search";

pub(crate) fn internal_specs() -> [ToolSpec; 2] {
    [
        ToolSpec {
            name: RECEIPT_READ.into(),
            description: "Read a bounded slice of the full durable result from a prior tool operation by operation id. Internal, read-only, no external effects.".into(),
            schema: json!({"type":"object","properties":{
                "id":{"type":"string","description":"Exact operation id returned by receipt search or observed in history"},
                "startChar":{"type":"integer","minimum":0,"description":"Unicode character offset, default 0"},
                "maxChars":{"type":"integer","minimum":128,"maximum":6000}
            },"required":["id"],"additionalProperties":false}),
            read_only: true,
        },
        ToolSpec {
            name: RECEIPT_SEARCH.into(),
            description: "Search all durable operations, including older omitted context, by tool name, operation id, arguments or result text. Returns bounded IDs and previews for receipt_read.".into(),
            schema: json!({"type":"object","properties":{
                "query":{"type":"string","description":"Literal case-insensitive keyword; omit to list newest operations"},
                "offset":{"type":"integer","minimum":0},
                "limit":{"type":"integer","minimum":1,"maximum":20}
            },"additionalProperties":false}),
            read_only: true,
        },
    ]
}

/// Old observations remain available via a bounded query, not a re-run of
/// the original external tool. These tools never consult outside a run.
pub(crate) fn execute_local(action: &Action, receipts: &[Receipt])
    -> Option<Result<Value, String>> {
    match action.tool.as_str() {
        RECEIPT_READ => Some(read_receipt(&action.arguments, receipts)),
        RECEIPT_SEARCH => Some(search_receipts(&action.arguments, receipts)),
        _ => None,
    }
}
fn nonnegative_number(args: &Value, key: &str, default: usize) -> Result<usize, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(value) => value.as_u64()
            .and_then(|number| usize::try_from(number).ok())
            .ok_or_else(|| format!("{key} must be a non-negative integer")),
    }
}
fn read_receipt(args: &Value, receipts: &[Receipt]) -> Result<Value, String> {
    let id = args.get("id").and_then(Value::as_str).filter(|id|
        !id.is_empty() && id.len() <= 128).ok_or("Valid receipt id required")?;
    let start = nonnegative_number(args, "startChar", 0)?;
    let limit = nonnegative_number(args, "maxChars", 3000)?.clamp(128,6000);
    let receipt=receipts.iter().find(|receipt|receipt.id==id)
        .ok_or("No matching receipt in this run")?;
    let full=receipt.result.to_string();
    let char_count=full.chars().count();
    if start > char_count {return Err("startChar exceeds receipt length".into());}
    let content=full.chars().skip(start).take(limit).collect::<String>();
    Ok(json!({"id":receipt.id,"tool":receipt.tool,"success":receipt.success,
        "step":receipt.step,"arguments":receipt.arguments,
        "totalChars":char_count,"startChar":start,"text":content,
        "nextChar":(start+limit<char_count).then_some(start+limit)}))
}
fn search_receipts(args: &Value, receipts: &[Receipt]) -> Result<Value, String> {
    let query=args.get("query").and_then(Value::as_str).unwrap_or("").trim();
    if query.chars().count()>128 {return Err("Receipt search query is too long".into());}
    let needle=query.to_lowercase();
    let offset=nonnegative_number(args,"offset",0)?;
    let limit=nonnegative_number(args,"limit",10)?.clamp(1,20);
    let mut matches=Vec::new();
    let mut total=0usize;
    for receipt in receipts.iter().rev() {
        let haystack=format!("{} {} {} {}",receipt.id,receipt.tool,
            receipt.arguments,receipt.result).to_lowercase();
        if !haystack.contains(&needle) {continue;}
        if total >= offset && matches.len() < limit {
            matches.push(json!({
                "id":receipt.id,"tool":receipt.tool,"step":receipt.step,
                "success":receipt.success,
                "arguments":clip(&receipt.arguments.to_string(),120,60),
                "resultPreview":clip(&receipt.result.to_string(),160,260),
            }));
        }
        total+=1;
    }
    Ok(json!({"query":query,"offset":offset,"total":total,
        "nextOffset":(offset+matches.len()<total).then_some(offset+matches.len()),
        "matches":matches}))
}

/// Keep beginning *and* end. Diagnostics commonly appear at the end of
/// compiler output; retaining only the head hides the actionable error.
pub(crate) fn clip(source:&str,head:usize,tail:usize)->String {
    let count=source.chars().count();
    if count<=head+tail+48 {return source.into();}
    let prefix=source.chars().take(head).collect::<String>();
    let suffix=source.chars().skip(count-tail).collect::<String>();
    format!("{prefix}\n[{} chars hidden; read durable receipt for full text]\n{suffix}",
        count-head-tail)
}
#[derive(Debug)]
pub(crate) struct Projection {
    pub recent: Vec<Value>,
    pub important_failures: Vec<Value>,
    pub history_index: Vec<Value>,
    pub digest: Value,
}
/// A bounded model-view of a complete, immutable receipt log. No operation
/// is deleted or merged in durable storage. A newer failure cannot disappear
/// merely because a dozen successful reads followed it.
pub(crate) fn project(receipts:&[Receipt], strategy:Strategy, max_result_chars:usize)
    -> Projection {
    let (count,cap)=match strategy {
        Strategy::LowCost=>(5usize,4500usize),
        Strategy::Balanced=>(10,10000),
        Strategy::VerifyFirst=>(16,16000),
    };
    let per_result=max_result_chars.clamp(128,3200);
    let recent_start=receipts.len().saturating_sub(count);
    let mut recent=Vec::new();
    let mut budget=cap*2/3;
    for receipt in receipts[recent_start..].iter().rev() {
        let full=receipt.result.to_string();
        let args=clip(&receipt.arguments.to_string(),180,100);
        // The LowCost viewport may be smaller than a single max-size tool
        // observation. Shrink that observation to fit rather than losing the
        // newest evidence *and every older receipt* after the first oversize.
        // Measure serialized JSON (escaped quotes/newlines count too).
        let mut allowance=per_result.min(budget.saturating_sub(96));
        let mut selected=None;
        for _ in 0..12 {
            if allowance<32 {break;}
            let result=clip(&full,allowance*2/5,allowance*3/5);
            let item=json!({"id":receipt.id,"tool":receipt.tool,"step":receipt.step,
                "success":receipt.success,"arguments":args,"result":result});
            let size=item.to_string().chars().count();
            if size<=budget {selected=Some((item,size));break;}
            allowance=allowance*3/4;
        }
        let Some((item,size))=selected else {break;};
        budget-=size;
        recent.push(item);
    }
    recent.reverse();
    let mut important_failures=Vec::new();
    let mut failure_budget=cap/6;
    for receipt in receipts[..recent_start].iter().rev().filter(|r|!r.success).take(4) {
        let item=json!({"id":receipt.id,"tool":receipt.tool,"step":receipt.step,
            "result":clip(&receipt.result.to_string(),200,400)});
        let size=item.to_string().chars().count();
        if size>failure_budget {break;}
        failure_budget-=size;
        important_failures.push(item);
    }
    important_failures.reverse();
    let mut history_index=Vec::new();
    let mut index_budget=cap/6;
    for receipt in receipts[..recent_start].iter().rev().take(160) {
        let item=json!({"id":receipt.id,"tool":receipt.tool,"success":receipt.success,
            "step":receipt.step});
        let size=item.to_string().chars().count();
        if size>index_budget {break;}
        index_budget-=size;
        history_index.push(item);
    }
    history_index.reverse();
    let mut by_tool=BTreeMap::<String,usize>::new();
    let mut paths=BTreeSet::<String>::new();
    for receipt in receipts {
        *by_tool.entry(receipt.tool.clone()).or_default()+=1;
        if let Some(path)=receipt.arguments["path"].as_str() {
            if paths.len()<32 && path.len()<=240 {paths.insert(path.into());}
        }
    }
    let digest=json!({
        "totalOperations":receipts.len(),
        "failedOperations":receipts.iter().filter(|r|!r.success).count(),
        "operationCounts":by_tool,
        "touchedPaths":paths.into_iter().collect::<Vec<_>>(),
        "olderObservationsOmitted":recent_start.saturating_sub(history_index.len()),
        "retrieveOlder":format!("Use {RECEIPT_SEARCH} then {RECEIPT_READ}; no prior tool is re-executed")
    });
    Projection {recent,important_failures,history_index,digest}
}

#[cfg(test)]
mod tests {
    use super::*;
    fn receipt(id:&str,step:usize,success:bool,result:Value)->Receipt {
        Receipt{id:id.into(),step,tool:"workspace_read".into(),
            arguments:json!({"path":format!("src/{id}.rs")}),success,result}
    }
    #[test]
    fn history_is_bounded_but_every_receipt_is_retrievable() {
        let data=(0..260).map(|i|receipt(&format!("r{i}"),i+1,true,
            json!({"content":"source".repeat(2000)}))).collect::<Vec<_>>();
        let p=project(&data,Strategy::Balanced,3000);
        let surfaced=serde_json::to_string(&p.recent).unwrap().len()
            +serde_json::to_string(&p.important_failures).unwrap().len()+serde_json::to_string(&p.history_index).unwrap().len();
        assert!(surfaced<17000,"projection must not grow with full log: {surfaced}");
        assert_eq!(data.len(),260);
        let search=execute_local(&Action{id:"s".into(),tool:RECEIPT_SEARCH.into(),
            arguments:json!({"query":"r0"}),depends_on:vec![]},&data).unwrap().unwrap();
        assert!(search["total"].as_u64().unwrap()>=1);
        let read=execute_local(&Action{id:"l".into(),tool:RECEIPT_READ.into(),
            arguments:json!({"id":"r0","maxChars":200}),depends_on:vec![]},&data).unwrap().unwrap();
        assert_eq!(read["id"],"r0");
        assert_eq!(read["text"].as_str().unwrap().chars().count(),200);
        assert!(read["nextChar"].is_number());
    }
    #[test]
    fn low_cost_large_latest_receipt_is_clipped_not_silently_discarded() {
        let data=(0..8).map(|i|receipt(&format!("r{i}"),i+1,true,
            json!({"stderr":format!("{}FINAL_DIAGNOSTIC","\\\"".repeat(1200))})))
            .collect::<Vec<_>>();
        let projection=project(&data,Strategy::LowCost,3200);
        assert!(!projection.recent.is_empty(),
            "a single large newest receipt must not empty the low-cost context");
        assert_eq!(projection.recent.last().unwrap()["id"],"r7");
        assert!(projection.recent.last().unwrap()["result"].to_string().contains("FINAL_DIAGNOSTIC"));
        let serialized=serde_json::to_string(&projection.recent).unwrap();
        assert!(serialized.chars().count()<4000,
            "bounded projection must respect the low-cost evidence budget");
    }

    #[test]
    fn preserve_failure_when_recent_successful_reads_drown_it_out() {
        let mut data=vec![receipt("failed-build",1,false,
            json!({"stderr":format!("{}REAL_ERROR", "x".repeat(5000))}))];
        data.extend((0..20).map(|i|receipt(&format!("r{i}"),i+2,true,json!("OK"))));
        let p=project(&data,Strategy::Balanced,800);
        assert!(serde_json::to_string(&p.important_failures).unwrap().contains("REAL_ERROR"));
    }
    #[test]
    fn newer_failures_and_unicode_boundaries_retain_tail() {
        let text=format!("{}끝나는_오류","가".repeat(2000));
        let fragment=clip(&text,30,40);
        assert!(fragment.contains("끝나는_오류"));
        let result=read_receipt(&json!({"id":"한글","startChar":1,"maxChars":128}),
            &[receipt("한글",1,false,json!({"error":text}))]).unwrap();
        assert_eq!(result["startChar"],1);
        assert!(result["text"].as_str().unwrap().chars().count()<=128);
    }
    #[test]
    fn search_pages_and_never_reads_other_run_ids() {
        let data=(0..6).map(|i|receipt(&format!("hit-{i}"),1,false,json!("X")))
            .collect::<Vec<_>>();
        let first=search_receipts(&json!({"query":"hit","limit":2}),&data).unwrap();
        assert_eq!(first["total"],6);
        assert_eq!(first["matches"].as_array().unwrap().len(),2);
        assert_eq!(first["nextOffset"],2);
        let second=search_receipts(&json!({"query":"hit","offset":2,"limit":2}),&data).unwrap();
        assert_ne!(first["matches"][0]["id"],second["matches"][0]["id"]);
        assert!(read_receipt(&json!({"id":"other-run"}),&data).is_err());
        assert!(search_receipts(&json!({"query":"x".repeat(129)}),&data).is_err());
    }
}
