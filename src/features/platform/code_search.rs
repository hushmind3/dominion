//! Optional repository-symbol inspection; not an engine primitive.
use crate::engine::tools::{ToolContext,resolve_workspace};
use serde_json::{json,Value};

pub(crate) fn search_code_index(
    ctx: &ToolContext<'_>,
    query: &str,
    limit: usize,
) -> Result<Value, String> {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return Err("code_search query는 비어 있을 수 없습니다.".into());
    }
    let meta=match crate::features::platform::code_index::get(ctx.state,ctx.workspace) {
        Some(meta)=>meta,
        None=>crate::features::platform::code_index::refresh(ctx.state,ctx.workspace)?,
    };
    let index_id = meta["id"].as_str().unwrap_or("");
    let terms=needle.split_whitespace().map(|term|term.trim_matches(['`','\"','\'','(',')',',']))
        .filter(|term|!term.is_empty() && !matches!(*term,"fn"|"pub"|"async"|"struct"|"enum"))
        .collect::<Vec<_>>();
    let score=|value:&str|{
        let value=value.to_lowercase();
        if value==needle {1000} else {
            u64::from(value.contains(&needle))*200 + terms.iter().map(|term|
                if value==*term {100} else {u64::from(value.contains(term))*20}).sum::<u64>()
        }
    };
    let mut matches=Vec::new();
    for file in ctx.store.all("code_index_file") {
        if file["indexId"].as_str()!=Some(index_id) {continue;}
        let path=file["path"].as_str().unwrap_or("");
        let rank=score(path);
        if rank>0 {matches.push((rank,json!({"path":path,"kind":"file","name":path,"line":1})));}
        for symbol in file["symbols"].as_array().into_iter().flatten() {
            let name=symbol["name"].as_str().unwrap_or("");let rank=score(name);
            if rank==0 {continue;}
            matches.push((rank,json!({"path":path,"kind":symbol["kind"],"name":name,"line":symbol["line"]})));
        }
    }
    // Literal diagnostics and field names often are not symbol names. Search
    // the actual workspace bytes when the index has no navigation hit.
    if matches.is_empty() {
        for file in ctx.store.all("code_index_file") {
            if file["indexId"].as_str()!=Some(index_id) {continue;}
            let Some(path)=file["path"].as_str() else {continue;};
                let Ok(full)=resolve_workspace(ctx.workspace,path) else {continue;};
            let Ok(content)=std::fs::read_to_string(full) else {continue;};
            for (index,line) in content.lines().enumerate().filter(|(_,line)|line.to_lowercase().contains(&needle)).take(limit) {
                matches.push((100,json!({"path":path,"kind":"literal","line":index+1,
                    "name":line.trim().chars().take(180).collect::<String>()})));
            }
        }
    }
    matches.sort_by(|(a,left),(b,right)|b.cmp(a)
        .then_with(||left["path"].as_str().cmp(&right["path"].as_str()))
        .then_with(||left["line"].as_u64().cmp(&right["line"].as_u64())));
    matches.dedup_by(|(_,a),(_,b)|a==b);
    matches.truncate(limit);
    let matches=matches.into_iter().map(|(_,row)|row).collect::<Vec<_>>();
    Ok(json!({"query":query,"indexId":index_id,"indexedFiles":meta["files"],"matchCount":matches.len(),"matches":matches}))
}


pub fn definitions()->Vec<Value>{
 json!([{"type":"function","function":{
  "name":"code_search",
  "description":"Search the persistent repository index for file paths, symbols and line numbers. Use this before reading a large file.",
  "parameters":{"type":"object","properties":{
   "query":{"type":"string","description":"Function, type, path or keyword fragment"},
   "limit":{"type":"integer","minimum":1,"maximum":50}
  },"required":["query"],"additionalProperties":false}
 }}]).as_array().unwrap().clone()
}
pub fn execute(ctx:&ToolContext<'_>,name:&str,args:&Value)->Option<Result<Value,String>>{
 if name!="code_search"{return None}
 Some((||{
  let query=args["query"].as_str().ok_or("Missing argument: query")?;
  let limit=args["limit"].as_u64().unwrap_or(20).clamp(1,50) as usize;
  search_code_index(ctx,query,limit)
 })())
}
#[cfg(test)]
mod tests{
 use super::*;
 #[test] fn code_index_search_is_optional_capability(){
  assert_eq!(definitions()[0]["function"]["name"],"code_search");
 }
}
