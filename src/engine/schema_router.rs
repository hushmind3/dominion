//! Deterministic two-stage tool-schema routing for the native Rust engine.
//! Full schemas for eligible MCP tools are loaded only when the task matches
//! their descriptions or an explicit tool_discover turn promotes them.
//! Source of authority remains the engine-owned, permission-scoped catalog.
//! Ranking uses no model call, embedding server, global cache, or API key.
use serde_json::{json,Value};
use std::collections::{HashMap,HashSet};

const MAX_INITIAL_MCP:usize=4;
const MAX_DISCOVER:usize=8;

#[derive(Debug)]
pub(super) struct Selection {
    pub tools:Vec<Value>,
    pub complete_bytes:usize,
    pub selected_bytes:usize,
    pub omitted:usize,
    pub selected_mcp:Vec<String>,
}

pub(super) fn name(value:&Value)->Option<&str>{
    value.get("function")?.get("name")?.as_str()
}
fn size(values:&[Value])->Result<usize,String>{
    serde_json::to_vec(values).map(|raw|raw.len())
        .map_err(|error|format!("tool schema JSON encoding: {error}"))
}
fn words(text:&str)->HashSet<String>{
    text.split(|ch:char|!ch.is_alphanumeric())
        .map(str::trim).filter(|word|word.chars().count()>=3)
        .map(str::to_lowercase).collect()
}
fn relevance(query:&str,tool:&Value)->u64{
    let Some(alias)=name(tool) else{return 0;};
    let query_lower=query.to_lowercase();
    let alias_lower=alias.to_lowercase();
    if query_lower.contains(&alias_lower){return 10_000;}
    let desc=tool["function"]["description"].as_str().unwrap_or("");
    let terms=words(query);
    if terms.is_empty(){return 0;}
    let alias_words=words(alias);
    let descriptor=words(desc);
    let matched_name=terms.intersection(&alias_words).count() as u64;
    let matched_desc=terms.intersection(&descriptor).count() as u64;
    matched_name.saturating_mul(10)+matched_desc.saturating_mul(2)
}
fn mcp_candidates(catalog:&[Value],query:&str)->Vec<(usize,u64)>{
    let mut scored=catalog.iter().enumerate().filter_map(|(i,tool)|{
        name(tool).filter(|name|name.starts_with("mcp__"))
            .map(|_|(i,relevance(query,tool)))
    }).collect::<Vec<_>>();
    scored.sort_by(|a,b|b.1.cmp(&a.1).then_with(||a.0.cmp(&b.0)));
    scored
}
pub(super) fn max_schema_bytes(context_limit:usize)->usize {
    // The protocol overhead and pinned owner instruction need the remainder.
    (context_limit/3).clamp(4096,16_384)
}
/// All native tools remain available. Optional MCP schemas are injected only
/// under the hard encoded-byte bound; explicit promotions take priority over
/// lexical matches, but cannot override the context limit.
pub(super) fn select(catalog:&[Value],query:&str,
    promoted:&HashSet<String>,max_bytes:usize)->Result<Selection,String>{
    let complete_bytes=size(catalog)?;
    let mut seen=HashSet::new();
    let mut tools=Vec::new();
    for item in catalog{
        let alias=name(item).ok_or("registered tool has no function name")?;
        if alias.trim().is_empty(){return Err("registered tool name is empty".into());}
        if !alias.starts_with("mcp__") && !seen.contains(alias){
            seen.insert(alias.to_owned());tools.push(item.clone());
        }
    }
    if size(&tools)?>max_bytes {
        return Err(format!("Mandatory native tool schemas exceed the bounded schema budget of {max_bytes} bytes"));
    }
    let mut extras=mcp_candidates(catalog,query);
    // Requests made by tool_discover win priority, even when a low lexical
    // score would otherwise drop them. This makes discovery functional.
    extras.sort_by(|a,b|{
        let pa=name(&catalog[a.0]).is_some_and(|n|promoted.contains(n));
        let pb=name(&catalog[b.0]).is_some_and(|n|promoted.contains(n));
        pb.cmp(&pa).then_with(||b.1.cmp(&a.1)).then_with(||a.0.cmp(&b.0))
    });
    let mut selected_mcp=Vec::new();
    let mut initial=0usize;
    for (i,score) in extras {
        let alias=name(&catalog[i]).expect("candidate must have name");
        if seen.contains(alias){continue;}
        let explicit=promoted.contains(alias);
        if !explicit {
            if score==0 || initial>=MAX_INITIAL_MCP {continue;}
        }
        let mut candidate=tools.clone();
        candidate.push(catalog[i].clone());
        if size(&candidate)?>max_bytes {
            if explicit {
                return Err(format!(
                    "Discovered tool '{alias}' cannot fit in the schema byte budget; refusing a partial invocation contract"
                ));
            }
            continue;
        }
        seen.insert(alias.to_owned());
        tools.push(catalog[i].clone());
        selected_mcp.push(alias.to_owned());
        if !explicit{initial+=1;}
    }
    let selected_bytes=size(&tools)?;
    Ok(Selection{
        omitted:catalog.len().saturating_sub(tools.len()),
        complete_bytes,selected_bytes,tools,selected_mcp,
    })
}

/// Browse only ALREADY PERMITTED external tools. Names/brief descriptions
/// are shown on demand. Returned aliases are promoted on the next model step
/// rather than executing the tool or handing an unbounded schema to the model.
pub(super) fn discover(catalog:&[Value],query:&str,limit:usize)->Value {
    let candidates=mcp_candidates(catalog,query);
    let count=candidates.len();
    let matched=candidates.iter().filter(|(_,score)|*score>0).count();
    let entries=candidates.into_iter()
        .take(limit.clamp(1,MAX_DISCOVER))
        .map(|(index,score)|{
            let item=&catalog[index];
            let alias=name(item).unwrap_or("");
            let description=item["function"]["description"].as_str()
                .unwrap_or("").chars().take(200).collect::<String>();
            json!({"name":alias,"description":description,"lexicalScore":score})
        }).collect::<Vec<_>>();
    json!({"tools":entries,"eligibleMcpTools":count,
        "matchingTools":matched,
        "note":"The listed tools become available with full argument schemas in the next model turn. If a tool is missing, search with a more specific name or English capability keyword."})
}

#[cfg(test)]
mod tests {
    use super::*;
    fn tool(alias:&str,description:&str,noise:usize)->Value{
        json!({"type":"function","function":{
            "name":alias,"description":description,
            "parameters":{"type":"object","properties":{
                "input":{"type":"string","description":"x".repeat(noise)}
            },"required":["input"]}
        }})
    }
    fn catalog()->Vec<Value>{
        let mut out=vec![tool("workspace_read","Read source code",1),
            tool("workspace_replace","Modify local source code",1),
            tool("tool_discover","Find eligible external tools",1)];
        for n in 0..50 {
            let description=if n==43 {"Find invoices and payment receipts"}
                else if n==12 {"Search flights and airline schedules"}
                else {"Unrelated independent external service"};
            out.push(tool(&format!("mcp__service__action{n}"),description,800));
        }
        out
    }
    #[test]
    fn schema_bytes_shrink_and_native_permissions_never_expand(){
        let all=catalog();
        let selected=select(&all,"Please read source code",
            &HashSet::new(),5500).unwrap();
        assert_eq!(selected.selected_mcp.len(),0);
        assert_eq!(selected.tools.len(),3);
        assert!(selected.complete_bytes>selected.selected_bytes*5);
        assert!(selected.selected_bytes<=5500);
    }
    #[test]
    fn matching_mcp_schema_is_available_without_loading_all_50(){
        let all=catalog();
        let selected=select(&all,"Find payment invoices",
            &HashSet::new(),5500).unwrap();
        assert!(selected.selected_mcp.iter().any(|n|n.ends_with("action43")));
        assert!(!selected.selected_mcp.iter().any(|n|n.ends_with("action12")));
        assert!(selected.selected_bytes<=5500);
        assert!(selected.omitted>=49);
    }
    #[test]
    fn tool_discovery_promotes_a_full_schema_next_turn(){
        let all=catalog();
        let found=discover(&all,"flights airline",4);
        let alias=found["tools"][0]["name"].as_str().unwrap();
        assert_eq!(alias,"mcp__service__action12");
        let mut promoted=HashSet::new();
        promoted.insert(alias.to_owned());
        let selected=select(&all,"unrelated current user instruction",
            &promoted,5500).unwrap();
        assert!(selected.selected_mcp.iter().any(|n|n==alias));
    }
    #[test]
    fn external_schema_budget_failure_never_silently_breaks_explicit_discovery(){
        let all=catalog();
        let mut promoted=HashSet::new();
        promoted.insert("mcp__service__action12".into());
        assert!(select(&all,"other",&promoted,500).is_err());
        assert!(select(&all,"other",&promoted,1900).is_err());
        let selected=select(&all,"other",&HashSet::new(),1900).unwrap();
        assert!(selected.selected_mcp.is_empty());
    }
    #[test]
    fn name_collision_prefers_native_and_cannot_be_invoked_twice(){
        let mut all=catalog();
        all.push(tool("workspace_read","forged builtin",100));
        let selected=select(&all,"read",&HashSet::new(),5500).unwrap();
        assert_eq!(selected.tools.iter().filter(|x|name(x)==Some("workspace_read")).count(),1);
    }
}
