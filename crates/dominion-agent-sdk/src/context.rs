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
            description: "Read a bounded slice of the full durable result or original arguments from a prior operation by id. The field argument accepts result (default) or arguments. Internal, read-only, no external effects.".into(),
            schema: json!({"type":"object","properties":{
                "id":{"type":"string","description":"Exact operation id returned by receipt search or observed in history"},
                "field":{"type":"string","enum":["result","arguments"],"description":"Which immutable receipt field to read, default result"},
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
    let field=args.get("field").and_then(Value::as_str).unwrap_or("result");
    let full=match field {
        "result"=>receipt.result.to_string(),
        "arguments"=>receipt.arguments.to_string(),
        _=>return Err("Receipt field must be result or arguments".into()),
    };
    let char_count=full.chars().count();
    if start > char_count {return Err("startChar exceeds receipt length".into());}
    let content=full.chars().skip(start).take(limit).collect::<String>();
    // 'maxChars' must not be defeated by a huge argument payload carried
    // alongside a bounded result. Keep the full immutable receipt in storage;
    // only replace oversized model-visible arguments with a short preview.
    let arguments_text=receipt.arguments.to_string();
    let arguments=if arguments_text.chars().count()>2048 {
        json!({"truncated":true,"totalChars":arguments_text.chars().count(),
            "preview":clip(&arguments_text,240,240)})
    } else {receipt.arguments.clone()};
    Ok(json!({"id":receipt.id,"tool":receipt.tool,"success":receipt.success,
        "step":receipt.step,"field":field,"arguments":arguments,
        "totalChars":char_count,"startChar":start,"text":content,
        "nextChar":(start+limit<char_count).then_some(start+limit)}))
}
fn search_receipts(args: &Value, receipts: &[Receipt]) -> Result<Value, String> {
    let query=args.get("query").and_then(Value::as_str).unwrap_or("").trim();
    if query.chars().count()>128 {return Err("Receipt search query is too long".into());}
    let needle=query.to_lowercase();
    let offset=nonnegative_number(args,"offset",0)?;
    let limit=nonnegative_number(args,"limit",10)?.clamp(1,20);
    let preview=|receipt:&Receipt| json!({
        "id":receipt.id,"tool":receipt.tool,"step":receipt.step,
        "success":receipt.success,
        "arguments":clip(&receipt.arguments.to_string(),120,60),
        "resultPreview":clip(&receipt.result.to_string(),160,260),
    });
    // The normal 'show recent history' call requires no keyword scan.
    // Restrict serialization to the requested page instead of materializing
    // every (potentially multi-megabyte) historic tool result.
    if needle.is_empty() {
        let total=receipts.len();
        let matches=receipts.iter().rev().skip(offset).take(limit)
            .map(preview).collect::<Vec<_>>();
        return Ok(json!({"query":query,"offset":offset,"total":total,
            "nextOffset":(offset+matches.len()<total).then_some(offset+matches.len()),
            "matches":matches}));
    }
    let mut matches=Vec::new();
    let mut total=0usize;
    for receipt in receipts.iter().rev() {
        let haystack=format!("{} {} {} {}",receipt.id,receipt.tool,
            receipt.arguments,receipt.result).to_lowercase();
        if !haystack.contains(&needle) {continue;}
        if total >= offset && matches.len() < limit {
            matches.push(preview(receipt));
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
        // Failed operations remain actionable even when diagnostics contain
        // huge escaped strings. A fixed-size preview can exceed the budget
        // after JSON serialization; shrink it instead of dropping the error.
        let full=receipt.result.to_string();
        let mut allowance=600usize.min(failure_budget.saturating_sub(96));
        let mut selected=None;
        for _ in 0..12 {
            if allowance<32 {break;}
            let item=json!({"id":receipt.id,"tool":receipt.tool,"step":receipt.step,
                "result":clip(&full,allowance*2/5,allowance*3/5)});
            let size=item.to_string().chars().count();
            if size<=failure_budget {selected=Some((item,size));break;}
            allowance=allowance*3/4;
        }
        if let Some((item,size))=selected {
            failure_budget-=size;
            important_failures.push(item);
        }
        // One oversized record must not hide every older failure.
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
    fn receipt_read_cannot_expand_unbounded_arguments_into_model_context() {
        let mut record=receipt("large-arguments",1,true,json!("short result"));
        record.arguments=json!({"content":"\\\"".repeat(20_000)});
        let answer=read_receipt(&json!({"id":"large-arguments","maxChars":128}),
            &[record.clone()]).unwrap();
        assert_eq!(answer["arguments"]["truncated"],true);
        assert!(answer["arguments"]["preview"].as_str().unwrap().len()<1300);
        assert!(answer.to_string().len()<2000,"bounded receipt must remain bounded");
        assert!(record.arguments["content"].as_str().unwrap().len()>20_000,
            "durable original arguments are never modified");
        let raw=record.arguments.to_string();
        let page=read_receipt(&json!({"id":"large-arguments","field":"arguments",
            "startChar":100,"maxChars":128}),&[record.clone()]).unwrap();
        assert_eq!(page["field"],"arguments");
        assert_eq!(page["totalChars"],raw.chars().count());
        assert_eq!(page["text"],raw.chars().skip(100).take(128).collect::<String>());
        assert_eq!(page["nextChar"],228);
        assert!(read_receipt(&json!({"id":"large-arguments","field":"untrusted"}),
            &[record]).is_err());
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
    fn escaped_oversized_failure_keeps_the_actionable_diagnostic() {
        let mut data=vec![receipt("huge-failure",1,false,
            json!({"stderr":format!("{}ROOT_CAUSE_AT_END", "\\\"".repeat(20_000))}))];
        data.extend((0..20).map(|i|receipt(&format!("read-{i}"),i+2,true,json!("OK"))));
        let projection=project(&data,Strategy::LowCost,3200);
        let failures=serde_json::to_string(&projection.important_failures).unwrap();
        assert!(failures.contains("huge-failure"),"must keep error identity");
        assert!(failures.contains("ROOT_CAUSE_AT_END"),"must keep diagnostic tail");
        assert!(failures.chars().count()<=4500/6,
            "failure index must remain inside its token budget");
    }
    #[test]
    fn one_unrepresentable_error_does_not_hide_smaller_old_failure() {
        let mut data=vec![
            receipt("older-error",1,false,json!("SMALL_ROOT_CAUSE")),
            receipt(&"x".repeat(2000),2,false,json!("oversized receipt identity"))
        ];
        data.extend((0..20).map(|i|receipt(&format!("read-{i}"),i+3,true,json!("OK"))));
        let projection=project(&data,Strategy::LowCost,3200);
        let failures=serde_json::to_string(&projection.important_failures).unwrap();
        assert!(failures.contains("SMALL_ROOT_CAUSE"));
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
    fn empty_query_lists_a_bounded_recent_page_without_full_log_projection() {
        let data=(0..150).map(|i|receipt(&format!("r{i}"),i+1,true,
            json!({"largeResult":"x".repeat(20_000)}))).collect::<Vec<_>>();
        let first=search_receipts(&json!({"limit":3}),&data).unwrap();
        assert_eq!(first["total"],150);
        assert_eq!(first["matches"][0]["id"],"r149");
        assert_eq!(first["nextOffset"],3);
        assert!(first["matches"][0]["resultPreview"].as_str().unwrap().len()<1000);
        let second=search_receipts(&json!({"offset":149,"limit":3}),&data).unwrap();
        assert_eq!(second["matches"][0]["id"],"r0");
        assert!(second["nextOffset"].is_null());
        let past_end=search_receipts(&json!({"offset":999,"limit":3}),&data).unwrap();
        assert!(past_end["matches"].as_array().unwrap().is_empty());
        assert_eq!(past_end["total"],150);
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
