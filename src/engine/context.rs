//! Budgeted model-facing context projection.
//! Never rewrites the durable session record or persisted tool receipts.
//! Preserve assistant tool-call groups atomically to avoid orphan tool results.
use serde_json::{json, Value};
use std::collections::HashSet;

#[derive(Debug, Clone)]
pub struct Projection {
    pub messages: Vec<Value>,
    pub original_chars: usize,
    pub projected_chars: usize,
    pub omitted_messages: usize,
}

fn chars(value: &Value) -> usize { value.to_string().chars().count() }
fn role(item: &Value) -> &str { item["role"].as_str().unwrap_or("") }
fn shorten(item: &Value) -> Value {
    let mut next=item.clone();
    if matches!(role(item),"tool"|"assistant") {
        if let Some(content)=item["content"].as_str() {
            let length=content.chars().count();
            if length>2400 {
                let head=content.chars().take(1400).collect::<String>();
                let tail=content.chars().skip(length.saturating_sub(600)).collect::<String>();
                next["content"]=json!(format!(
                    "{head}\n[{} characters omitted in this model request; inspect persisted receipt when needed]\n{tail}",
                    length.saturating_sub(2000)
                ));
            }
        }
    }
    next
}
fn groups(messages: &[Value]) -> Vec<Vec<Value>> {
    let mut result: Vec<Vec<Value>>=Vec::new();
    let mut outstanding: HashSet<String>=HashSet::new();
    for message in messages {
        if role(message)=="tool" {
            let id=message["tool_call_id"].as_str().unwrap_or("");
            if outstanding.remove(id) && !result.is_empty() {
                result.last_mut().unwrap().push(message.clone());
                continue;
            }
        }
        outstanding.clear();
        if role(message)=="assistant" {
            if let Some(calls)=message["tool_calls"].as_array() {
                for call in calls {
                    if let Some(id)=call["id"].as_str() { outstanding.insert(id.into()); }
                }
            }
        }
        result.push(vec![message.clone()]);
    }
    result
}
/// Project complete ordered exchanges rather than independently removing messages.
/// Each model request is bounded without changing local history or receipts.
pub fn project(input: &[Value], max_chars: usize) -> Projection {
    let original_chars=input.iter().map(chars).sum();
    if input.len()<=2 || original_chars<=max_chars {
        return Projection{messages:input.to_vec(),original_chars,projected_chars:original_chars,omitted_messages:0};
    }
    let prefix_count=input.iter().take_while(|m|role(m)=="system").count();
    let mut prefix=input[..prefix_count].to_vec();
    let rest=groups(&input[prefix_count..]);
    let mut selected: Vec<Vec<Value>>=Vec::new();
    let mut selected_count=0usize;
    let prefix_chars=prefix.iter().map(chars).sum::<usize>();
    // Reserve room for a short notice about omitted history.
    let mut cost=prefix_chars+500;
    let ceiling=max_chars.max(prefix_chars+4000);
    for group in rest.iter().rev() {
        let reduced=group.iter().map(shorten).collect::<Vec<_>>();
        let needed=reduced.iter().map(chars).sum::<usize>();
        // The newest complete exchange is mandatory. Subsequent exchanges
        // may not evict a more recent owner directive or leave tool output orphaned.
        if !selected.is_empty() && cost.saturating_add(needed)>ceiling {break;}
        cost=cost.saturating_add(needed);
        selected.push(reduced);
        selected_count+=1;
        if selected_count>=32 {break;}
    }
    selected.reverse();
    let dropped=&rest[..rest.len().saturating_sub(selected_count)];
    let dropped_count=dropped.iter().map(Vec::len).sum::<usize>();
    if dropped_count>0 {
        // The notice is metadata about omitted content, not an invented user turn.
        prefix.push(json!({"role":"system","content":format!(
            "Context window projection: {dropped_count} earlier records were omitted from this request. They remain in durable task events. Re-read source and tool receipts before asserting their results. The most recent owner instruction overrides stale context."
        )}));
    }
    prefix.extend(selected.into_iter().flatten());
    let projected_chars=prefix.iter().map(chars).sum();
    Projection{messages:prefix,original_chars,projected_chars,omitted_messages:dropped_count}
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn leaves_small_history_unchanged() {
        let original=vec![json!({"role":"system","content":"s"}),
            json!({"role":"user","content":"read"})];
        let projection=project(&original,8000);
        assert_eq!(projection.messages,original);
    }
    #[test]
    fn multiple_tool_results_remain_with_their_assistant_call() {
        let mut history=vec![json!({"role":"system","content":"s"})];
        for index in 0..18 {
            history.push(json!({"role":"assistant","content":null,"tool_calls":[
                {"id":format!("a{index}"),"type":"function","function":{"name":"workspace_read","arguments":"{}"}},
                {"id":format!("b{index}"),"type":"function","function":{"name":"workspace_read","arguments":"{}"}}
            ]}));
            history.push(json!({"role":"tool","tool_call_id":format!("a{index}"),"content":"a".repeat(1200)}));
            history.push(json!({"role":"tool","tool_call_id":format!("b{index}"),"content":"b".repeat(1200)}));
        }
        history.push(json!({"role":"user","content":"Latest owner direction remains visible"}));
        let untouched=history.clone();
        let reduced=project(&history,11000);
        assert_eq!(history,untouched);
        assert!(reduced.omitted_messages>0);
        assert!(reduced.messages.last().unwrap().to_string().contains("Latest owner direction"));
        let mut known=HashSet::new();
        for item in &reduced.messages {
            if let Some(calls)=item["tool_calls"].as_array() {
                for call in calls {
                    if let Some(id)=call["id"].as_str(){known.insert(id.to_string());}
                }
            }
            if role(item)=="tool" {
                let id=item["tool_call_id"].as_str().unwrap();
                assert!(known.contains(id), "orphan receipt: {id}");
            }
        }
    }
}
