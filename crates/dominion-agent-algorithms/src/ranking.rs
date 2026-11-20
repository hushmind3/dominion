//! Pure ranking signals: metadata relevance only, never execution authority.
use serde_json::Value;
use std::collections::BTreeSet;

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
/// A second, bounded retrieval view over JSON parameter *field names*.
/// Domain-specific parameters often identify MCP capabilities more precisely
/// than generic connector names (e.g. "execute" with "issue_number").
/// The schema is only ranking evidence, never a new dispatch permission.
/// Bounded depth and vocabulary prevent large or recursive schemas from
/// consuming unbounded matching work.
fn schema_vocabulary(schema:&Value,depth:usize,fields:&mut BTreeSet<String>){
 if depth>=4||fields.len()>=96{return}
 let Some(object)=schema.as_object() else{return};
 if let Some(properties)=object.get("properties").and_then(Value::as_object){
  for (key,subschema) in properties {
   for word in words(key){if fields.len()<96{fields.insert(word);}}
   schema_vocabulary(subschema,depth+1,fields);
   if fields.len()>=96{break}
  }
 }
 if let Some(items)=object.get("items"){schema_vocabulary(items,depth+1,fields);}
}
pub fn schema_relevance(schema:&Value,query:&str)->i32{
 let mut fields=BTreeSet::new();
 schema_vocabulary(schema,0,&mut fields);
 let query=words(&enrich(query));
 // Generic transport parameters do not distinguish one MCP capability
 // from another. Ignore them as schema-only ranking signals.
 query.into_iter().filter(|token|
  !matches!(token.as_str(),"id"|"name"|"value"|"data"|"args"|"payload"|
   "input"|"output"|"limit"|"count"|"query"|"text"|"type"|"page")
  &&fields.contains(token)).take(6).count() as i32*4
}
/// Two-view lexical fusion: stable tool name/description relevance plus
/// bounded parameter-name relevance. Shared by native job and chat discovery.
/// Unlike a remote embedding call, this is deterministic and has no token cost.
pub fn tool_relevance_with_schema(
 name:&str,description:&str,schema:&Value,query:&str
)->i32{
 tool_relevance(name,description,query).saturating_add(schema_relevance(schema,query))
}
