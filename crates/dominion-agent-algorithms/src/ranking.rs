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
  // Korean requests use postpositions and native service names. Surface the
  // matching authorized connector in the FIRST planning turn, without an
  // embedding call or a synthetic grant of execution authority.
  ("깃허브"," github repository issue pull request branch commit"),
  ("깃헙"," github repository issue pull request branch"),
  ("기트허브"," github repository issue pull request"),
  ("레포지토리"," github repository"),
  ("리포지토리"," github repository"),
  ("풀리퀘스트"," github pull request"),
  ("풀 리퀘스트"," github pull request"),
  ("이메일"," gmail email message inbox"),
  ("메일"," gmail email message inbox"),
  ("받은편지함"," gmail email inbox"),
  ("구글 드라이브"," google drive file"),
  ("구글드라이브"," google drive file"),
  ("구글 문서"," google docs document"),
  ("구글독스"," google docs document"),
  ("캘린더"," calendar event schedule"),
  ("회의 일정"," calendar event schedule"),
  ("일정 등록"," calendar event schedule"),
  ("클라우드플레어"," cloudflare worker dns"),
  ("레일웨이"," railway deploy application"),
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
  // Two-character abbreviations like PR and CI must match entire tokens.
  // Substring scoring on "pr" accidentally promoted prompt/provider tools.
  else if token.chars().count()>=3 && name.contains(&token){s+=7}
  else if token.chars().count()>=3 && desc.contains(&token){s+=2}
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

#[cfg(test)]
mod localization_tests {
    use super::*;
    #[test]
    fn korean_owner_service_names_resolve_to_authorized_metadata() {
        assert!(tool_relevance("github_work_read",
            "Read GitHub issues and pull requests",
            "깃허브에서 열린 풀리퀘스트 목록을 조사해 주세요") >
            tool_relevance("google_workspace_read",
            "Read Gmail and Drive documents",
            "깃허브에서 열린 풀리퀘스트 목록을 조사해 주세요"));
        assert!(tool_relevance("google_workspace_read",
            "Search connected Gmail messages",
            "이메일 받은편지함에서 지난 답장을 찾아줘") >
            tool_relevance("github_work_read",
            "Read GitHub issues", "이메일 받은편지함에서 지난 답장을 찾아줘"));
        assert!(tool_relevance("google_workspace_read",
            "Calendar events from connected Google services",
            "캘린더에 회의 일정 등록해") >
            tool_relevance("github_work_read",
            "Read GitHub issues", "캘린더에 회의 일정 등록해"));
        assert!(tool_relevance("cloudflare_deploy",
            "Cloudflare Worker deployments",
            "클라우드플레어에서 배포 결과 봐줘") >
            tool_relevance("railway_deploy",
            "Railway deployment", "클라우드플레어에서 배포 결과 봐줘"));
    }

    #[test]
    fn short_pr_abbreviation_does_not_select_unrelated_prefix_tools() {
        assert_eq!(tool_relevance("prompt_response",
            "Send a provider prompt", "PR"), 0);
        assert!(tool_relevance("github_pr_read",
            "Read pull requests", "PR") > 0);
    }
}
