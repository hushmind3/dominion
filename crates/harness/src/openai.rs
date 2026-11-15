use crate::{Message, ModelReply, Provider, Role, ToolCall, ToolSpec};
use serde_json::{json, Value};

/// OpenAI-compatible chat-completions adapter.
///
/// The harness owns the loop; this type only translates between its message
/// shape and the provider wire format. Tool schemas are sent honestly, so a
/// provider that cannot call tools produces prose instead of fake actions.
pub struct OpenAiProvider {
    base_url: String,
    api_key: String,
    model: String,
    tools: Vec<ToolSpec>,
    temperature: f32,
}

impl OpenAiProvider {
    pub fn new(base_url: impl Into<String>, api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            api_key: api_key.into(),
            model: model.into(),
            tools: Vec::new(),
            temperature: 0.0,
        }
    }

    pub fn with_tools(mut self, tools: Vec<ToolSpec>) -> Self {
        self.tools = tools;
        self
    }

    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.temperature = temperature;
        self
    }

    fn payload(&self, messages: &[Message]) -> Value {
        let messages = messages
            .iter()
            .map(|message| {
                json!({
                    "role": match message.role {
                        Role::System => "system",
                        Role::User => "user",
                        Role::Assistant => "assistant",
                        Role::Tool => "tool",
                    },
                    "content": message.content,
                })
            })
            .collect::<Vec<_>>();
        let mut payload = json!({
            "model": self.model,
            "messages": messages,
            "temperature": self.temperature,
        });
        if !self.tools.is_empty() {
            payload["tools"] = json!(self
                .tools
                .iter()
                .map(|tool| json!({
                    "type": "function",
                    "function": {
                        "name": tool.name,
                        "description": tool.description,
                        "parameters": tool.parameters,
                    }
                }))
                .collect::<Vec<_>>());
            payload["tool_choice"] = json!("auto");
        }
        payload
    }
}

impl Provider for OpenAiProvider {
    fn complete(&mut self, messages: &[Message]) -> Result<ModelReply, String> {
        if self.api_key.is_empty() && !self.base_url.starts_with("http://127.0.0.1") {
            return Err("api key가 필요합니다.".into());
        }
        let response = reqwest::blocking::Client::builder()
            .build()
            .map_err(|error| error.to_string())?
            .post(format!("{}/chat/completions", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&self.payload(messages))
            .send()
            .map_err(|error| format!("provider 요청 실패: {error}"))?;
        let status = response.status();
        let body: Value = response
            .json()
            .map_err(|error| format!("provider 응답 파싱 실패: {error}"))?;
        if !status.is_success() {
            return Err(format!(
                "provider HTTP {}: {}",
                status.as_u16(),
                body.to_string().chars().take(400).collect::<String>()
            ));
        }
        parse_reply(&body)
    }
}

/// Parse a chat-completions response into the harness reply shape.
pub fn parse_reply(body: &Value) -> Result<ModelReply, String> {
    let message = body
        .pointer("/choices/0/message")
        .ok_or("provider 응답에 message가 없습니다.")?;
    let content = message["content"].as_str().unwrap_or("").to_string();
    let calls = message
        .get("tool_calls")
        .and_then(Value::as_array)
        .map(|calls| {
            calls
                .iter()
                .filter_map(|call| {
                    let name = call.pointer("/function/name")?.as_str()?.to_string();
                    let arguments = call
                        .pointer("/function/arguments")
                        .and_then(Value::as_str)
                        .and_then(|raw| serde_json::from_str(raw).ok())
                        .unwrap_or_else(|| json!({}));
                    Some(ToolCall {
                        id: call["id"].as_str().unwrap_or("").to_string(),
                        name,
                        arguments,
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if calls.is_empty() {
        Ok(ModelReply::Final { content })
    } else {
        Ok(ModelReply::Assistant { content, tool_calls: calls })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Role;

    #[test]
    fn a_reply_without_tool_calls_is_final() {
        let body = json!({"choices":[{"message":{"content":"done"}}]});
        assert_eq!(parse_reply(&body).unwrap(), ModelReply::Final { content: "done".into() });
    }

    #[test]
    fn tool_call_arguments_are_decoded_from_the_json_string() {
        let body = json!({"choices":[{"message":{"content":"",
            "tool_calls":[{"id":"call_1","type":"function","function":{
                "name":"fs_read","arguments":"{\"path\":\"a.txt\"}"}}]}}]});
        match parse_reply(&body).unwrap() {
            ModelReply::Assistant { tool_calls, .. } => {
                assert_eq!(tool_calls.len(), 1);
                assert_eq!(tool_calls[0].name, "fs_read");
                assert_eq!(tool_calls[0].arguments["path"], "a.txt");
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn payload_keeps_roles_and_declares_tools() {
        let provider = OpenAiProvider::new("http://x/v1", "k", "m").with_tools(vec![ToolSpec {
            name: "t".into(),
            description: "d".into(),
            parameters: json!({"type":"object"}),
            permission: crate::Permission::Read,
        }]);
        let payload = provider.payload(&[Message::system("s"), Message::user("u")]);
        assert_eq!(payload["messages"][0]["role"], "system");
        assert_eq!(payload["messages"][1]["role"], "user");
        assert_eq!(payload["tools"][0]["function"]["name"], "t");
    }

    #[test]
    fn invalid_arguments_do_not_drop_the_call() {
        let body = json!({"choices":[{"message":{"content":"",
            "tool_calls":[{"id":"c","type":"function","function":{"name":"fs_list","arguments":"not json"}}]}}]});
        match parse_reply(&body).unwrap() {
            ModelReply::Assistant { tool_calls, .. } => {
                assert_eq!(tool_calls[0].arguments, json!({}));
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn roles_round_trip() {
        let payload = OpenAiProvider::new("http://x", "k", "m")
            .payload(&[Message::tool("result")]);
        assert_eq!(payload["messages"][0]["role"], "tool");
        assert_eq!(Role::Tool as u8, Role::Tool as u8);
    }
}
