//! Bounded, model-independent progressive tool disclosure.
//!
//! Provider tool schemas are a *view* over the owner-approved host catalog.
//! The full catalog remains private to Rust dispatch and never becomes a
//! second permission table. Discovery only returns names from that catalog.
use crate::{Action,Receipt,ToolSpec};
use serde_json::{json,Value};
use std::collections::{BTreeSet,HashMap};

pub(crate) const SEARCH: &str="dominion_tool_search";
const THRESHOLD:usize=18;
const VISIBLE:usize=13;

pub(crate) fn spec()->ToolSpec {
 ToolSpec{
  name:SEARCH.into(),
  description:"Discover owner-authorized capabilities by intent. Search before assuming an unavailable tool; matching tool schemas are enabled in the NEXT model turn. Internal and read-only.".into(),
  schema:json!({"type":"object","properties":{
   "query":{"type":"string","minLength":1,"maxLength":256,
      "description":"Describe the missing action, service, or tool capability"},
   "limit":{"type":"integer","minimum":1,"maximum":8}
  },"required":["query"],"additionalProperties":false}),
  read_only:true,
 }
}
pub(crate) fn needed(catalog:&HashMap<String,ToolSpec>)->bool {
 catalog.len()>THRESHOLD
}
fn words(s:&str)->BTreeSet<String>{
 s.to_lowercase().split(|c:char|!c.is_alphanumeric())
  .filter(|w|w.chars().count()>=2).map(str::to_owned).collect()
}
fn enrich(text:&str)->String {
 let lower=text.to_lowercase();
 let mut s=lower.clone();
 for (need,aliases) in [
  ("코드"," workspace code file read replace edit shell"),
  ("파일"," workspace file read write replace"),
  ("구현"," code workspace replace write test"),
  ("수정"," code replace file write edit"),
  ("개발"," workspace code test shell"),
  ("테스트"," shell exec test"),
  ("검색"," web search http fetch"),
  ("조사"," web search http fetch"),
  ("브라우저"," browser control"),
  ("영상"," video render"),
  ("위임"," delegate task"),
  ("代码"," code workspace read replace"),
  ("修复"," fix file replace shell"),
  ("搜索"," search web http"),
  ("浏览器"," browser control"),
  ("测试"," test shell exec"),
 ] {if lower.contains(need){s.push_str(aliases)}}
 s
}
fn score(spec:&ToolSpec, query:&str)->i32{
 let query=enrich(query);
 let tokens=words(&query);
 let name=spec.name.to_lowercase();
 let name_words=words(&name);
 let desc=spec.description.to_lowercase();
 let mut s=0;
 if query.contains(&name){s+=32}
 for token in tokens {
  if name_words.contains(&token){s+=12}
  else if name.contains(&token){s+=7}
  else if desc.contains(&token){s+=2}
 }
 s
}
fn ranked(catalog:&HashMap<String,ToolSpec>,query:&str)->Vec<String>{
 let mut candidates=catalog.values()
  .filter(|s|s.name!=SEARCH && !s.name.starts_with("dominion_receipt_"))
  .map(|s|(score(s,query),s.name.clone())).collect::<Vec<_>>();
 candidates.sort_by(|a,b|b.0.cmp(&a.0).then_with(||a.1.cmp(&b.1)));
 candidates.into_iter().map(|(_,name)|name).collect()
}
/// Deterministic stable shortlist. Internal discovery receives no permissions
/// beyond the already-authorized catalog, and the original host owns dispatch.
pub(crate) fn select(catalog:&HashMap<String,ToolSpec>,task:&str,owner:&str,
 directive:Option<&str>,plan:&[String],receipts:&[Receipt])->Vec<ToolSpec>{
 if !needed(catalog){
  let mut all=catalog.values().cloned().collect::<Vec<_>>();
  all.sort_by(|a,b|a.name.cmp(&b.name));
  return all
 }
 let mut displayed=BTreeSet::new();
 displayed.insert(SEARCH.to_string());
 if !receipts.is_empty() {
  for name in ["dominion_receipt_search","dominion_receipt_read"] {
   if catalog.contains_key(name){displayed.insert(name.into());}
  }
 }
 // Promotion is tied to an actual successful kernel-local discovery receipt.
 for receipt in receipts.iter().rev().filter(|r|r.tool==SEARCH&&r.success).take(3) {
  if let Some(matches)=receipt.result["matches"].as_array(){
   for item in matches.iter().take(8) {
    if let Some(name)=item["name"].as_str(){
     if displayed.len()<VISIBLE && catalog.contains_key(name){
      displayed.insert(name.into());
     }
    }
   }
  }
 }
 // Keep tools used successfully in recent actions visible for follow-up
 // edits; failed tools can still be recovered through explicit discovery.
 for r in receipts.iter().rev().take(8) {
  if displayed.len()<VISIBLE && r.success && catalog.contains_key(&r.tool){
   displayed.insert(r.tool.clone());
  }
 }
 let mut query=format!("{task} {owner} {}",directive.unwrap_or(""));
 for part in plan.iter().take(6){query.push(' ');query.push_str(part);}
 for name in ranked(catalog,&query) {
  if displayed.len()>=VISIBLE{break}
  displayed.insert(name);
 }
 displayed.into_iter().filter_map(|name|catalog.get(&name).cloned()).collect()
}
pub(crate) fn execute_local(action:&Action,catalog:&HashMap<String,ToolSpec>)
 -> Option<Result<Value,String>> {
 if action.tool!=SEARCH{return None}
 Some(search(&action.arguments,catalog))
}
fn search(args:&Value,catalog:&HashMap<String,ToolSpec>)->Result<Value,String>{
 let query=args["query"].as_str().filter(|s|!s.trim().is_empty()
  && s.chars().count()<=256).ok_or("Tool discovery requires a query of 1-256 characters")?;
 let limit=match args.get("limit"){
  None|Some(Value::Null)=>5,
  Some(v)=>v.as_u64().filter(|n|(1..=8).contains(n))
   .ok_or("Tool discovery limit must be 1-8")? as usize
 };
 let chosen=ranked(catalog,query).into_iter().take(limit)
  .filter_map(|name|catalog.get(&name))
  .map(|spec|json!({"name":spec.name,"description":
    spec.description.chars().take(170).collect::<String>(),
    "readOnly":spec.read_only}))
  .collect::<Vec<_>>();
 Ok(json!({"query":query,"authorizedToolCount":catalog.len().saturating_sub(1),
  "matches":chosen,"nextTurnSchemas":true,
  "note":"These are already owner-approved host tools. Their full schemas appear in the next model turn."}))
}

#[cfg(test)]
mod tests{
 use super::*;
 fn catalog()->HashMap<String,ToolSpec>{
  let mut tools=(0..80).map(|i|{
   let name=format!("mcp__generic_{i:03}");
   (name.clone(),ToolSpec{name,description:"Generic unrelated endpoint".into(),
    schema:json!({"type":"object","properties":{"payload":{"type":"string"}}}),
    read_only:false})
  }).collect::<HashMap<_,_>>();
  for name in ["workspace_read_range","workspace_replace_range","shell_exec",
   "web_search","mcp__archive_publish","dominion_receipt_read"] {
   tools.insert(name.into(),ToolSpec{name:name.into(),
    description:format!("Execute {name}"),schema:json!({}),read_only:false});
  }
  tools.insert(SEARCH.into(),spec());
  tools
 }
 #[test]
 fn shortlist_is_bounded_reproducible_and_keeps_owner_permissions_unchanged(){
  let c=catalog();
  let a=select(&c,"코드 수정 후 테스트","",None,&[],&[]);
  let b=select(&c,"코드 수정 후 테스트","",None,&[],&[]);
  assert_eq!(a.iter().map(|s|&s.name).collect::<Vec<_>>(),
             b.iter().map(|s|&s.name).collect::<Vec<_>>());
  assert!(a.len()<=VISIBLE);
  assert!(a.iter().any(|s|s.name=="dominion_tool_search"));
  assert!(a.iter().any(|s|s.name=="shell_exec"));
  assert!(a.iter().any(|s|s.name=="workspace_replace_range"));
  let full=serde_json::to_string(&c).unwrap().len();
  let short=serde_json::to_string(&a).unwrap().len();
  assert!(short*2<full,"catalog should shrink without paid retrieval");
 }
 #[test]
 fn tool_search_promotes_only_real_authorized_catalog_entries(){
  let c=catalog();
  let a=Action{id:"lookup".into(),tool:SEARCH.into(),
    arguments:json!({"query":"archive publish","limit":3}),depends_on:vec![]};
  let reply=execute_local(&a,&c).unwrap().unwrap();
  assert!(reply["matches"].as_array().unwrap().iter()
    .any(|r|r["name"]=="mcp__archive_publish"));
  let receipt=Receipt{id:"lookup".into(),tool:SEARCH.into(),
    arguments:a.arguments,success:true,result:reply,step:1};
  let visible=select(&c,"Read a repository","",None,&[],&[receipt]);
  assert!(visible.iter().any(|s|s.name=="mcp__archive_publish"));
  assert!(search(&json!({"query":"","limit":99}),&c).is_err());
  assert!(visible.iter().all(|s|c.contains_key(&s.name)));
 }
 #[test]
 fn small_catalog_stays_fully_visible_without_new_discovery_rounds(){
  let mut c=HashMap::new();
  for key in ["read","write"] {
   c.insert(key.into(),ToolSpec{name:key.into(),description:String::new(),
     schema:json!({}),read_only:key=="read"});
  }
  assert_eq!(select(&c,"anything","",None,&[],&[]).len(),2);
  assert!(!needed(&c));
 }
}
