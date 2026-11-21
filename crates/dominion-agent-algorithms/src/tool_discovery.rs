//! Bounded, model-independent progressive tool disclosure.
//!
//! Provider tool schemas are a *view* over the owner-approved host catalog.
//! The full catalog remains private to Rust dispatch and never becomes a
//! second permission table. Discovery only returns names from that catalog.
use crate::{failure_guidance,Action,Receipt,ToolSpec};
use serde_json::{json,Value};
use std::collections::{BTreeSet,HashMap};

pub const SEARCH: &str="dominion_tool_search";
pub const CALL: &str="dominion_tool_call";
const THRESHOLD:usize=18;
const VISIBLE:usize=13;

pub fn spec()->ToolSpec {
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
pub fn call_spec()->ToolSpec {
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
pub fn needed(catalog:&HashMap<String,ToolSpec>)->bool {
 catalog.len()>THRESHOLD
}
// Relevance/ranking lives in the replaceable intelligence crate; the Rust
// dispatcher still owns the allowed catalog and the discovery permission fence.
use crate::ranking::{tool_relevance,tool_relevance_with_schema};
#[cfg(test)] use crate::ranking::schema_relevance;
fn score(spec:&ToolSpec, query:&str)->i32{
 tool_relevance_with_schema(&spec.name,&spec.description,&spec.schema,query)
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
pub fn select(catalog:&HashMap<String,ToolSpec>,task:&str,owner:&str,
 directive:Option<&str>,_plan:&[String],receipts:&[Receipt])->Vec<ToolSpec>{
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
 // Model-authored plans are untrusted, mutable observations. Feeding their
 // new keywords into provider declarations changes the tool-schema prefix
 // after every planning turn, invalidating KV caching and allowing a hidden
 // effect tool to surface without an explicit discovery receipt. Keep the
 // native list anchored to the owner goal/directive; evolving plans use the
 // fixed search+call bridge, which still verifies the owner-authorized schema.

 let query=format!("{task} {owner} {}",directive.unwrap_or(""));
 for name in ranked(catalog,&query) {
  if displayed.len()>=VISIBLE{break}
  displayed.insert(name);
 }
 displayed.into_iter().filter_map(|name|catalog.get(&name).cloned()).collect()
}
pub fn execute_local(action:&Action,catalog:&HashMap<String,ToolSpec>)
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
pub fn resolve_bridge(action:&Action,catalog:&HashMap<String,ToolSpec>,
 receipts:&[Receipt])->Result<Option<Action>,String>{
 if action.tool!=CALL{return Ok(None)}
 let name=action.arguments["name"].as_str()
  .filter(|s|!s.is_empty()&&s.len()<=128)
  .ok_or("Bridge calls require a valid discovered tool name")?;
 if name==CALL||name==SEARCH||name.starts_with("dominion_receipt_"){
  return Err("Internal tools cannot be invoked through the external tool bridge".into())
 }
 let spec=catalog.get(name)
  .ok_or_else(||format!("Tool is not authorized by the owner: {name}"))?;
 // Bind a recovered discovery receipt to its exact authorized tool contract.
 // The 2026 MCP tool list can change between discovery and execution. Name
 // alone is not evidence that an updated schema or effect policy was seen.
 let description=spec.description.chars().take(170).collect::<String>();
 let found=receipts.iter().any(|r|r.tool==SEARCH && r.success
  && r.result["matches"].as_array().is_some_and(|list|list.iter().any(|item|
   item["name"].as_str()==Some(name)
   &&item["parameters"]==spec.schema
   &&item["readOnly"].as_bool()==Some(spec.read_only)
   &&item["description"].as_str()==Some(description.as_str()))));
 if !found {return Err("Tool not discovered with its current schema and effect policy; call dominion_tool_search again".into())}
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
  let resolved=resolve_bridge(&bridge,&c,&[receipt.clone()]).unwrap().unwrap();
  assert_eq!(resolved.tool,"mcp__archive_publish");
  assert_eq!(resolved.arguments,json!({"id":"example"}));
  let mut schema_drift=c.clone();
  schema_drift.get_mut("mcp__archive_publish").unwrap().schema=
   json!({"type":"object","properties":{"different_id":{"type":"string"}}});
  assert!(resolve_bridge(&bridge,&schema_drift,&[receipt.clone()]).is_err(),
   "stale discovery cannot bind a changed tool schema");
  let mut policy_drift=c.clone();
  policy_drift.get_mut("mcp__archive_publish").unwrap().read_only=true;
  assert!(resolve_bridge(&bridge,&policy_drift,&[receipt.clone()]).is_err(),
   "stale discovery cannot bind a changed effect policy");
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
 fn model_plan_does_not_rewrite_native_prefix_and_hidden_effect_uses_bridge(){
  let c=catalog();
  let before=select(&c,"Review a market report","",None,&[],&[]);
  assert!(!before.iter().any(|tool|tool.name=="mcp__archive_publish"));
  let revised=select(&c,"Review a market report","",None,
   &["publish artifact to archive".into()],&[]);
  let names=|list:&[ToolSpec]|list.iter().map(|tool|tool.name.clone()).collect::<Vec<_>>();
  assert_eq!(names(&revised),names(&before),
      "untrusted model plans must not invalidate the native tool prefix");
  assert!(revised.len()<=VISIBLE);
  // The new plan still has access to the SAME authorized hidden effect tool
  // through the fixed search+call bridge and a real discovery receipt.
  let lookup=Action{id:"lookup-plan".into(),tool:SEARCH.into(),
      arguments:json!({"query":"archive publish"}),depends_on:vec![]};
  let result=execute_local(&lookup,&c).unwrap().unwrap();
  assert!(result["matches"].as_array().unwrap().iter()
      .any(|entry|entry["name"]=="mcp__archive_publish"));
  let receipt=Receipt{id:lookup.id.clone(),tool:lookup.tool.clone(),
      arguments:lookup.arguments.clone(),success:true,result,step:1};
  let bridge=Action{id:"publish-plan".into(),tool:CALL.into(),
      arguments:json!({"name":"mcp__archive_publish","arguments":{}}),
      depends_on:vec!["lookup-plan".into()]};
  assert_eq!(resolve_bridge(&bridge,&c,&[receipt]).unwrap().unwrap().tool,
      "mcp__archive_publish");
  // An explicit NEW owner directive may select a different bounded prefix.
  let corrected=select(&c,"Review a market report","",
   Some("Inspect code instead of publishing"),
   &["publish artifact to archive".into()],&[]);
  assert!(!corrected.iter().any(|tool|tool.name=="mcp__archive_publish"));
  assert!(corrected.iter().any(|tool|tool.name=="workspace_read_range"));
  let inspected=select(&c,"Publish a report","",Some("Inspect code"),
      &["publish artifact to archive".into()],&[]);
  assert!(inspected.iter().any(|tool|tool.name=="workspace_read_range"));
 }
 #[test]
 fn native_and_chat_share_only_the_metadata_ranking_not_permissions(){
  let c=catalog();
  let target=c.get("workspace_read_range").unwrap();
  assert_eq!(score(target,"Inspect code"),
   tool_relevance(&target.name,&target.description,"Inspect code"));
  assert!(tool_relevance("workspace_read_range","Read workspace code",
   "Inspect code")>tool_relevance("unrelated_end","Movie upload","Inspect code"));
  assert_eq!(tool_relevance("","", "Inspect code"),0);
 }
 #[test]
 fn schema_fields_recover_generic_mcp_capabilities_without_granting_others(){
  let mut c=catalog();
  let name="mcp__connector__execute_007";
  c.insert(name.into(),ToolSpec{
   name:name.into(),description:"Execute selected connector operation".into(),
   schema:json!({"type":"object","properties":{
    "repository_name":{"type":"string"},
    "issue_number":{"type":"integer"},
    "options":{"type":"object","properties":{"issue_state":{"type":"string"}}}
   }}),read_only:true,
  });
  let irrelevant="mcp__connector__execute_008";
  c.insert(irrelevant.into(),ToolSpec{
   name:irrelevant.into(),description:"Execute selected connector operation".into(),
   schema:json!({"type":"object","properties":{"video_frames":{"type":"array"}}}),
   read_only:false,
  });
  let q="repository issue";
  assert!(score(&c[name],q)>score(&c[irrelevant],q));
  assert!(score(&c[name],q)>
   tool_relevance(name,&c[name].description,q));
  let found=search(&json!({"query":q,"limit":5}),&c).unwrap();
  let names=found["matches"].as_array().unwrap();
  assert!(names.iter().any(|item|item["name"]==name));
  assert!(!names.iter().any(|item|item["name"]==irrelevant));
  let shortlist=select(&c,q,"",None,&[],&[]);
  assert!(shortlist.iter().any(|item|item.name==name));
  assert!(shortlist.iter().all(|item|c.contains_key(&item.name)));
  // Querying a hidden or unapproved name cannot create an execution grant.
  let forged=Action{id:"forged".into(),tool:CALL.into(),
   arguments:json!({"name":"mcp__unapproved","arguments":{}}),
   depends_on:vec![]};
  assert!(resolve_bridge(&forged,&c,&[]).is_err());
 }
 #[test]
 fn schema_ranking_is_depth_and_common_parameter_bounded(){
  let deep=json!({"properties":{"a":{"properties":{"b":{"properties":{
   "c":{"properties":{"d":{"properties":{"deep_secret":{"type":"string"}}}}}
  }}}}}});
  assert_eq!(schema_relevance(&deep,"deep secret"),0);
  let generic=json!({"properties":{"payload":{"type":"string"},
   "id":{"type":"string"},"name":{"type":"string"}}});
  assert_eq!(schema_relevance(&generic,"payload id name"),0);
  assert!(schema_relevance(&json!({"properties":{"issue_number":{"type":"integer"}}}),
   "issue number")>0);
 }
 #[test]
 fn korean_github_tasks_disclose_right_connector_without_a_paid_search_turn(){
  let mut c=catalog();
  for (name,description) in [
    ("github_work_read","Read GitHub issues, branches and pull requests"),
    ("google_workspace_read","Read Gmail, Calendar and Drive resources"),
    ("cloudflare_deploy","Deploy Cloudflare Workers"),
  ] {
    c.insert(name.into(),ToolSpec{name:name.into(),
      description:description.into(),schema:json!({"type":"object","properties":{
        "operation":{"type":"string"}}}),read_only:true});
  }
  let picked=select(&c,"깃허브에서 열린 풀리퀘스트를 조사해","",
       None,&[],&[]);
  assert!(picked.iter().any(|s|s.name=="github_work_read"));
  assert!(!picked.iter().any(|s|s.name=="google_workspace_read"));
  assert!(!picked.iter().any(|s|s.name=="cloudflare_deploy"));
  assert!(picked.len()<=VISIBLE);
  let search_result=search(&json!({"query":"깃허브 풀리퀘스트","limit":3}),&c).unwrap();
  assert_eq!(search_result["matches"][0]["name"],"github_work_read");
  assert!(picked.iter().all(|s|c.contains_key(&s.name)));
  // A hint cannot grant a connector absent from the owner-authorized set.
  c.remove("github_work_read");
  assert!(!select(&c,"깃허브에서 열린 풀리퀘스트","",None,&[],&[])
    .iter().any(|s|s.name=="github_work_read"));
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
