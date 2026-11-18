//! Bounded, model-independent progressive tool disclosure.
//!
//! Provider tool schemas are a *view* over the owner-approved host catalog.
//! The full catalog remains private to Rust dispatch and never becomes a
//! second permission table. Discovery only returns names from that catalog.
use crate::{failure_guidance,Action,Receipt,ToolSpec};
use serde_json::{json,Value};
use std::collections::{BTreeSet,HashMap};

pub(crate) const SEARCH: &str="dominion_tool_search";
pub(crate) const CALL: &str="dominion_tool_call";
const THRESHOLD:usize=18;
const VISIBLE:usize=13;

pub(crate) fn spec()->ToolSpec {
 ToolSpec{
  name:SEARCH.into(),
  description:"Discover owner-authorized capabilities by intent. Search before assuming an unavailable tool; full JSON schemas are returned in the search evidence for use with dominion_tool_call. Internal and read-only.".into(),
  schema:json!({"type":"object","properties":{
   "query":{"type":"string","minLength":1,"maxLength":256,
      "description":"Describe the missing action, service, or tool capability"},
   "limit":{"type":"integer","minimum":1,"maximum":8}
  },"required":["query"],"additionalProperties":false}),
  read_only:true,
 }
}
/// Fixed provider-visible bridge. Hidden tools are never added to the native
/// function declarations: search returns their schemas as tool evidence,
/// then the bridge resolves their invocation through ordinary host approval.
pub(crate) fn call_spec()->ToolSpec {
 ToolSpec{
  name:CALL.into(),
  description:"Invoke a previously discovered owner-approved tool by exact name, using its retrieved JSON parameters. Rust verifies the discovery receipt and dispatches through the normal approval and durable tool worker.".into(),
  schema:json!({"type":"object","properties":{
   "name":{"type":"string","description":"Exact authorized tool name from dominion_tool_search"},
   "arguments":{"type":"object","description":"Arguments conforming to that tool's retrieved JSON schema"}
  },"required":["name","arguments"],"additionalProperties":false}),
  read_only:false,
 }
}
pub(crate) fn needed(catalog:&HashMap<String,ToolSpec>)->bool {
 catalog.len()>THRESHOLD
}
fn words(s:&str)->BTreeSet<String>{
 s.to_lowercase().split(|c:char|!c.is_alphanumeric())
  .filter(|w|w.chars().count()>=2 && !matches!(*w,
   "the"|"and"|"for"|"with"|"this"|"that"|"from"|"your"|"you"|
   "use"|"using"|"can"|"will"|"need"|"after"|"into"|"then"|
   "are"|"was"|"have"|"has"|"should"|"not"|"please"|"all"|
   "tool"|"tools"|"service" // services are indistinguishable without domain nouns
  )).map(str::to_owned).collect()
}
fn enrich(text:&str)->String {
 let lower=text.to_lowercase();
 let mut s=lower.clone();
 for (need,aliases) in [
  ("코드"," workspace code file read replace edit shell"),
  ("inspect"," workspace read range code search"),
  ("code"," workspace read range code search"),
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
/// Model-independent relevance signal reused by native SDK and chat.
/// This is a ranking hint ONLY; it never changes owner tool authorization,
/// host approval, schema admission or durable effect execution.
pub fn tool_relevance(name:&str,description:&str,query:&str)->i32{
 let query=enrich(query);
 let tokens=words(&query);
 let name=name.to_lowercase();
 let name_words=words(&name);
 let desc=description.to_lowercase();
 let mut s=0;
 if !name.is_empty() && query.contains(&name){s+=32}
 for token in tokens {
  if name_words.contains(&token){s+=12}
  else if name.contains(&token){s+=7}
  else if desc.contains(&token){s+=2}
 }
 s
}
fn score(spec:&ToolSpec, query:&str)->i32{
 tool_relevance(&spec.name,&spec.description,query)
}
fn ranked(catalog:&HashMap<String,ToolSpec>,query:&str)->Vec<String>{
 let mut candidates=catalog.values()
  .filter(|s|s.name!=SEARCH && s.name!=CALL && !s.name.starts_with("dominion_receipt_"))
  .map(|s|(score(s,query),s.name.clone())).collect::<Vec<_>>();
 candidates.sort_by(|a,b|b.0.cmp(&a.0).then_with(||a.1.cmp(&b.1)));
 // A nonmatch is not a recommendation: avoid arbitrary alphabetic
 // schemas that dilute model attention and hide the discovery bridge.
 candidates.into_iter().filter(|(score,_)|*score>0)
  .map(|(_,name)|name).collect()
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
 displayed.insert(CALL.to_string());
 // The selected provider tool-list is stable until the owner changes the
 // goal or directive. Search results are DATA, not edits to provider schemas.
 // Durable receipts give the bridge authority to call exactly discovered tools.
 for name in ["dominion_receipt_search","dominion_receipt_read"]{
  if displayed.len()<VISIBLE&&catalog.contains_key(name){displayed.insert(name.into());}
 }
 // Prioritize at most two read-only probes already in the owner's catalog.
 // This is a diagnostic preference, not a new permission or tool execution.
 if let Some(hint)=failure_guidance::latest(receipts){
  for name in hint.probe_tools.iter().take(2){
   if displayed.len()>=VISIBLE{break;}
   if catalog.get(*name).is_some_and(|spec|spec.read_only){
    displayed.insert((*name).into());
   }
  }
 }
 // Reuse the *existing* model plan as a discovery hint, not as execution
 // authority. A later owner correction supersedes the old plan immediately.
 // This lets the model change strategies after observed tool results without
 // adding another planner, runtime, router or paid discovery call.
 if directive.is_none() {
  let plan_query=plan.iter().rev().take(3)
   .map(|step|step.chars().take(160).collect::<String>())
   .collect::<Vec<_>>().join(" ");
  if !plan_query.trim().is_empty() {
   let plan_quota=displayed.len().saturating_add(3).min(VISIBLE);
   for name in ranked(catalog,&plan_query) {
    if displayed.len()>=plan_quota{break}
    displayed.insert(name);
   }
  }
 }
 let query=format!("{task} {owner} {}",directive.unwrap_or(""));
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
    "readOnly":spec.read_only,"parameters":spec.schema}))
  .collect::<Vec<_>>();
 Ok(json!({"query":query,"authorizedToolCount":catalog.len().saturating_sub(1),
  "matches":chosen,"nextTurnSchemas":false,
  "note":"Use dominion_tool_call with the exact discovered name and JSON arguments. The provider tool list stays fixed; Rust preserves normal authorization."}))
}


/// Resolve a discovered call without changing the host permissions or
/// persisting a fictitious wrapper effect. The original action ID is retained
/// so crash recovery and dependency edges continue to refer to one operation.
pub(crate) fn resolve_bridge(action:&Action,catalog:&HashMap<String,ToolSpec>,
 receipts:&[Receipt])->Result<Option<Action>,String>{
 if action.tool!=CALL{return Ok(None)}
 let name=action.arguments["name"].as_str()
  .filter(|s|!s.is_empty()&&s.len()<=128)
  .ok_or("Bridge calls require a valid discovered tool name")?;
 if name==CALL||name==SEARCH||name.starts_with("dominion_receipt_"){
  return Err("Internal tools cannot be invoked through the external tool bridge".into())
 }
 if !catalog.contains_key(name) {
  return Err(format!("Tool is not authorized by the owner: {name}"))
 }
 let found=receipts.iter().any(|r|r.tool==SEARCH && r.success
  && r.result["matches"].as_array().is_some_and(|list|list.iter()
    .any(|item|item["name"].as_str()==Some(name))));
 if !found {return Err("Tool not previously discovered in this execution; call dominion_tool_search first".into())}
 let arguments=action.arguments["arguments"].as_object()
  .ok_or("Bridge arguments must be a JSON object matching the retrieved schema")?;
 Ok(Some(Action{id:action.id.clone(),tool:name.into(),
  arguments:Value::Object(arguments.clone()),depends_on:action.depends_on.clone()}))
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
  tools.insert(CALL.into(),call_spec());
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
  assert!(a.iter().any(|s|s.name=="dominion_tool_call"));
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
  let visible=select(&c,"Read a repository","",None,&[],&[receipt.clone()]);
  // Discovery never mutates the provider function-declaration prefix.
  assert_eq!(visible.iter().map(|s|&s.name).collect::<Vec<_>>(),
     select(&c,"Read a repository","",None,&[],&[]).iter().map(|s|&s.name).collect::<Vec<_>>());
  let bridge=Action{id:"publish".into(),tool:CALL.into(),
    arguments:json!({"name":"mcp__archive_publish","arguments":{"id":"example"}}),
    depends_on:vec!["lookup".into()]};
  let resolved=resolve_bridge(&bridge,&c,&[receipt]).unwrap().unwrap();
  assert_eq!(resolved.tool,"mcp__archive_publish");
  assert_eq!(resolved.arguments,json!({"id":"example"}));
  assert!(resolve_bridge(&bridge,&c,&[]).is_err());
  let unknown=Action{arguments:json!({"name":"unapproved","arguments":{}}),..bridge};
  assert!(resolve_bridge(&unknown,&c,&[]).is_err());
  assert!(search(&json!({"query":"","limit":99}),&c).is_err());
  assert!(visible.iter().all(|s|c.contains_key(&s.name)));
 }
 #[test]
 fn missing_path_promotes_only_authorized_read_only_inspection(){
  let mut c=catalog();
  c.insert("workspace_list".into(),ToolSpec{name:"workspace_list".into(),
   description:"Workspace inventory".into(),schema:json!({}),read_only:true});
  c.insert("code_search".into(),ToolSpec{name:"code_search".into(),
   description:"Symbol lookup".into(),schema:json!({}),read_only:true});
  let before=select(&c,"Analyze market risk","",None,&[],&[]);
  assert!(!before.iter().any(|s|s.name=="workspace_list"));
  let receipt=Receipt{id:"missing".into(),tool:"workspace_read".into(),
   arguments:json!({"path":"src/none.rs"}),success:false,
   result:json!({"error":"No such file or directory"}),step:1};
  let after=select(&c,"Analyze market risk","",None,&[],&[receipt.clone()]);
  assert!(after.iter().any(|s|s.name=="workspace_list"));
  assert!(after.iter().any(|s|s.name=="code_search"));
  assert!(after.len()<=VISIBLE);
  c.get_mut("workspace_list").unwrap().read_only=false;
  let forbidden=select(&c,"Analyze market risk","",None,&[],&[receipt]);
  assert!(!forbidden.iter().any(|s|s.name=="workspace_list"));
 }
 #[test]
 fn revised_plan_discovers_a_different_authorized_tool_without_a_second_loop(){
  let c=catalog();
  let before=select(&c,"Review a market report","",None,&[],&[]);
  assert!(!before.iter().any(|tool|tool.name=="mcp__archive_publish"));
  let revised=select(&c,"Review a market report","",None,
   &["publish artifact to archive".into()],&[]);
  assert!(revised.iter().any(|tool|tool.name=="mcp__archive_publish"));
  assert!(revised.len()<=VISIBLE);
  assert!(revised.iter().all(|tool|c.contains_key(&tool.name)));
  // An explicit new owner directive cannot be overridden by the stale plan.
  let corrected=select(&c,"Review a market report","",
   Some("Inspect code instead of publishing"),
   &["publish artifact to archive".into()],&[]);
  assert!(!corrected.iter().any(|tool|tool.name=="mcp__archive_publish"));
  assert!(corrected.iter().any(|tool|tool.name=="workspace_read_range"));
  // A direct follow-up must rediscover the relevant inspection tool even
  // when the original objective was unrelated and the old plan was to publish.
  let inspected=select(&c,"Publish a report","",Some("Inspect code"),
      &["publish artifact to archive".into()],&[]);
  assert!(inspected.iter().any(|tool|tool.name=="workspace_read_range"));
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
