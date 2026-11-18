//! Incremental provider-native SSE decoding and tool-call assembly.
//!
//! Transport byte boundaries never define event boundaries. Native calls are
//! executable ONLY after a complete terminal provider event, validated JSON
//! arguments, and a subsequent independent Rust-kernel approval.
use serde_json::{json,Value};
use std::collections::{BTreeMap,BTreeSet};

const MAX_LINE_BYTES:usize=256*1024;
const MAX_EVENT_BYTES:usize=2*1024*1024;
const MAX_TOTAL_BYTES:usize=32*1024*1024;
const MAX_NATIVE_CALLS:usize=80;
const MAX_ARGUMENT_BYTES:usize=1024*1024;

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub(crate) enum Wire {Chat,Anthropic,Codex}

#[derive(Debug)]
pub(crate) enum Packet {Data(Value),Done}

#[derive(Default)]
pub(crate) struct SseDecoder {
    line:Vec<u8>, event:Vec<u8>, total:usize, done:bool,
}
impl SseDecoder {
    /// Emits only blank-line-terminated SSE events, even if the transport
    /// splits a multibyte Unicode scalar or a JSON argument between chunks.
    pub(crate) fn push(&mut self, bytes:&[u8])->Result<Vec<Packet>,String>{
        self.total=self.total.checked_add(bytes.len())
            .filter(|n|*n<=MAX_TOTAL_BYTES)
            .ok_or("Native SSE transport exceeded 32 MiB")?;
        if self.done && !bytes.is_empty(){return Err("Data after terminal SSE [DONE]".into());}
        let mut packets=Vec::new();
        for &byte in bytes {
            if byte!=b'\n' {
                self.line.push(byte);
                if self.line.len()>MAX_LINE_BYTES {
                    return Err("Native SSE event line exceeded 256 KiB".into());
                }
                continue;
            }
            if self.line.last()==Some(&b'\r') {self.line.pop();}
            let line=std::str::from_utf8(&self.line)
                .map_err(|_|"Invalid UTF-8 inside SSE event")?;
            if line.is_empty(){
                if !self.event.is_empty(){
                    let raw=std::str::from_utf8(&self.event)
                        .map_err(|_|"Invalid SSE JSON UTF-8")?;
                    let packet=if raw.trim()=="[DONE]" {
                        self.done=true;Packet::Done
                    }else {
                        Packet::Data(serde_json::from_str(raw)
                            .map_err(|e|format!("Invalid native SSE JSON: {e}"))?)
                    };
                    packets.push(packet);
                    self.event.clear();
                }
            }else if let Some(data)=line.strip_prefix("data:"){
                if !self.event.is_empty(){self.event.push(b'\n');}
                self.event.extend_from_slice(data.strip_prefix(' ').unwrap_or(data).as_bytes());
                if self.event.len()>MAX_EVENT_BYTES {
                    return Err("Native SSE event exceeded 2 MiB".into());
                }
            }
            self.line.clear();
        }
        Ok(packets)
    }
    pub(crate) fn end(&self)->Result<(),String> {
        if !self.line.is_empty() || !self.event.is_empty(){
            return Err("Native stream disconnected during an incomplete SSE frame".into());
        }
        Ok(())
    }
}
#[derive(Default,Debug)]
struct Call {id:String,name:String,args:String}
impl Call {
    fn append(&mut self,value:&Value)->Result<(),String> {
        if let Some(id)=value["id"].as_str(){
            if self.id.len()+id.len()>256{return Err("Native call ID exceeds limit".into());}
            self.id.push_str(id);
        }
        if let Some(name)=value["function"]["name"].as_str(){
            if self.name.len()+name.len()>256{return Err("Native call name exceeds limit".into());}
            self.name.push_str(name);
        }
        if let Some(args)=value["function"]["arguments"].as_str(){
            if self.args.len()+args.len()>MAX_ARGUMENT_BYTES {
                return Err("Native call argument stream exceeds 1 MiB".into());
            }
            self.args.push_str(args);
        }
        Ok(())
    }
}
#[derive(Debug)]
enum Block {Text(String),Tool{id:String,name:String,partial:String,initial:Value},Ignored}
#[derive(Debug)]
pub(crate) enum Update {
    Text(String),
    ToolStarted {id:String,name:String},
}
#[derive(Debug)]
pub(crate) struct Assembler {
    wire:Wire,
    done:bool,
    chat_finish:Option<String>,
    text:String,
    calls:BTreeMap<usize,Call>,
    blocks:BTreeMap<usize,Block>,
    closed:BTreeSet<usize>,
    prompt_tokens:Option<u64>,
    output_tokens:Option<u64>,
    cached_read_tokens:Option<u64>,
    cached_write_tokens:Option<u64>,
    anthropic_stop:Option<String>,
    codex_response:Option<Value>,
}
impl Assembler {
    pub(crate) fn new(wire:Wire)->Self {
        Self {wire,done:false,chat_finish:None,text:String::new(),
            calls:BTreeMap::new(),blocks:BTreeMap::new(),closed:BTreeSet::new(),
            prompt_tokens:None,output_tokens:None,
            cached_read_tokens:None,cached_write_tokens:None,
            anthropic_stop:None,codex_response:None}
    }
    pub(crate) fn feed(&mut self,packet:Packet)->Result<Vec<Update>,String>{
        if self.done{return Err("Native stream delivered events after terminal completion".into());}
        let mut updates=Vec::new();
        let event=match packet {
            Packet::Done=>{
                if self.wire!=Wire::Chat {
                    return Err("Unexpected [DONE] for non-Chat native stream".into());
                }
                self.done=true;
                return Ok(updates);
            }
            Packet::Data(value)=>value,
        };
        match self.wire {
            Wire::Chat=>self.chat(&event,&mut updates)?,
            Wire::Anthropic=>self.anthropic(&event,&mut updates)?,
            Wire::Codex=>self.codex(&event,&mut updates)?,
        }
        Ok(updates)
    }
    fn chat(&mut self,event:&Value,updates:&mut Vec<Update>)->Result<(),String>{
        if let Some(tokens)=event["usage"]["prompt_tokens"].as_u64(){
            self.prompt_tokens=Some(tokens);
        }
        if let Some(tokens)=event["usage"]["completion_tokens"].as_u64(){
            self.output_tokens=Some(tokens);
        }
        if let Some(cached)=event["usage"]["prompt_tokens_details"]["cached_tokens"].as_u64()
            .or_else(||event["usage"]["prompt_token_details"]["cached_tokens"].as_u64()) {
            self.cached_read_tokens=Some(cached);
        }
        for choice in event["choices"].as_array().into_iter().flatten(){
            if choice["index"].as_u64().unwrap_or(0)!=0 {continue;}
            // OpenAI-compatible gateways can repeat the same terminal
            // marker before [DONE] (often alongside the final usage packet).
            // Treat only a NO-OP repetition as idempotent; never accept a
            // conflicting stop reason or new text/tool bytes after finishing.
            let reason=choice["finish_reason"].as_str();
            if let Some(previous)=self.chat_finish.as_deref(){
                if let Some(next)=reason {
                    if next!=previous {
                        return Err(format!("Conflicting Chat finish events: {previous} then {next}"));
                    }
                }
                if choice["delta"]["content"].as_str().is_some_and(|content|!content.is_empty())
                    || choice["delta"]["tool_calls"].as_array().is_some_and(|tools|!tools.is_empty()) {
                    return Err("Chat delivered content or tool arguments after finish event".into());
                }
                continue;
            }
            if let Some(reason)=reason {
                match reason {
                    "stop"|"tool_calls"=>{
                        self.chat_finish=Some(reason.into());
                        if reason=="tool_calls" {
                            for call in self.calls.values() {
                                if !call.id.is_empty() && !call.name.is_empty() {
                                    updates.push(Update::ToolStarted{
                                        id:call.id.clone(),name:call.name.clone()
                                    });
                                }
                            }
                        }
                    }
                    _=>return Err(format!("Native Chat stopped without a complete tool turn: {reason}")),
                }
            }
            if let Some(delta)=choice["delta"]["content"].as_str(){
                self.append_text(delta,updates)?;
            }
            for tool in choice["delta"]["tool_calls"].as_array().into_iter().flatten(){
                let idx=tool["index"].as_u64().ok_or("Native Chat tool delta has no index")?;
                let idx=usize::try_from(idx).map_err(|_|"Native Chat tool index overflow")?;
                if idx>=MAX_NATIVE_CALLS {return Err("Native tool call index exceeds limit".into());}
                let entry=self.calls.entry(idx).or_default();
                entry.append(tool)?;
            }
        }
        Ok(())
    }
    fn append_text(&mut self,delta:&str,updates:&mut Vec<Update>)->Result<(),String>{
        if self.text.len().saturating_add(delta.len())>MAX_EVENT_BYTES{
            return Err("Native assistant text exceeds 2 MiB".into());
        }
        self.text.push_str(delta);
        if !delta.is_empty(){updates.push(Update::Text(delta.into()));}
        Ok(())
    }
    fn anthropic(&mut self,event:&Value,updates:&mut Vec<Update>)->Result<(),String>{
        match event["type"].as_str().unwrap_or("") {
            "message_start"=>{
                let usage=&event["message"]["usage"];
                self.prompt_tokens=usage["input_tokens"].as_u64();
                self.cached_read_tokens=usage["cache_read_input_tokens"].as_u64();
                self.cached_write_tokens=usage["cache_creation_input_tokens"].as_u64();
            }
            "content_block_start"=>{
                let idx=event["index"].as_u64().ok_or("Anthropic content block missing index")?;
                let idx=usize::try_from(idx).map_err(|_|"Anthropic block index overflow")?;
                if idx>=MAX_NATIVE_CALLS{return Err("Anthropic content block index too large".into());}
                if self.blocks.contains_key(&idx){return Err("Duplicate Anthropic block index".into());}
                let block=&event["content_block"];
                let entry=match block["type"].as_str() {
                    Some("text")=>Block::Text(block["text"].as_str().unwrap_or("").into()),
                    Some("thinking"|"redacted_thinking")=>Block::Ignored,
                    Some("tool_use")=>{
                        let id=block["id"].as_str().filter(|v|!v.is_empty())
                            .ok_or("Anthropic native tool missing ID")?.to_string();
                        let name=block["name"].as_str().filter(|v|!v.is_empty())
                            .ok_or("Anthropic native tool missing name")?.to_string();
                        updates.push(Update::ToolStarted{id:id.clone(),name:name.clone()});
                        Block::Tool{id,name,partial:String::new(),initial:block["input"].clone()}
                    }
                    _=>return Err("Unknown Anthropic content block in agent stream".into()),
                };
                self.blocks.insert(idx,entry);
            }
            "content_block_delta"=>{
                let idx=event["index"].as_u64().ok_or("Anthropic content delta missing index")? as usize;
                if self.closed.contains(&idx) {
                    return Err("Anthropic delta after content block stop".into());
                }
                let block=self.blocks.get_mut(&idx).ok_or("Anthropic delta without block start")?;
                match (block,event["delta"]["type"].as_str()) {
                    (Block::Ignored,_)=>{},
                    (Block::Text(text),Some("text_delta"))=>{
                        let delta=event["delta"]["text"].as_str().unwrap_or("");
                        if text.len()+delta.len()>MAX_EVENT_BYTES{return Err("Anthropic text too long".into());}
                        text.push_str(delta);
                        if !delta.is_empty(){updates.push(Update::Text(delta.into()));}
                    }
                    (Block::Tool{partial,..},Some("input_json_delta"))=>{
                        let delta=event["delta"]["partial_json"].as_str().unwrap_or("");
                        if partial.len()+delta.len()>MAX_ARGUMENT_BYTES {
                            return Err("Anthropic tool arguments exceed 1 MiB".into());
                        }
                        partial.push_str(delta);
                    }
                    _=>return Err("Anthropic delta does not match content block".into()),
                }
            }
            "content_block_stop"=>{
                let idx=event["index"].as_u64().ok_or("Anthropic block stop missing index")? as usize;
                if !self.blocks.contains_key(&idx) || !self.closed.insert(idx) {
                    return Err("Duplicate or unknown Anthropic block stop".into());
                }
            }
            "message_delta"=>{
                if let Some(reason)=event["delta"]["stop_reason"].as_str(){
                    match reason {
                        "end_turn"|"tool_use"=>self.anthropic_stop=Some(reason.into()),
                        _=>return Err(format!("Anthropic native generation stopped: {reason}")),
                    }
                }
                if let Some(output)=event["usage"]["output_tokens"].as_u64(){
                    self.output_tokens=Some(output);
                }
                if let Some(cached)=event["usage"]["cache_read_input_tokens"].as_u64(){
                    self.cached_read_tokens=Some(cached);
                }
                if let Some(cached)=event["usage"]["cache_creation_input_tokens"].as_u64(){
                    self.cached_write_tokens=Some(cached);
                }
            }
            "message_stop"=>{
                if self.anthropic_stop.is_none(){
                    return Err("Anthropic message ended without stop reason".into());
                }
                if self.blocks.len()!=self.closed.len(){
                    return Err("Anthropic message ended with an unclosed tool/content block".into());
                }
                self.done=true;
            }
            "ping"=>{},
            "error"=>return Err(format!("Anthropic native stream failed: {}",event["error"])),
            _=>{},
        }
        Ok(())
    }
    fn codex(&mut self,event:&Value,updates:&mut Vec<Update>)->Result<(),String>{
        match event["type"].as_str().unwrap_or("") {
            "response.output_text.delta"=>{
                let delta=event["delta"].as_str().unwrap_or("");
                self.append_text(delta,updates)?;
            }
            "response.output_item.added"=>{
                let item=&event["item"];
                if item["type"]=="function_call"{
                    if let (Some(id),Some(name))=(item["call_id"].as_str(),
                        item["name"].as_str()){
                        updates.push(Update::ToolStarted{id:id.into(),name:name.into()});
                    }
                }
            }
            "response.completed"=>{
                let response=&event["response"];
                if response["status"]!="completed" {
                    return Err("Codex response.completed has non-completed status".into());
                }
                self.prompt_tokens=response["usage"]["input_tokens"].as_u64();
                self.output_tokens=response["usage"]["output_tokens"].as_u64();
                self.codex_response=Some(response.clone());
                self.done=true;
            }
            "response.failed"|"response.incomplete"|"error"=>{
                return Err(format!("Codex native stream failed: {}",
                    event["response"]["error"]["message"].as_str()
                        .or_else(||event["error"]["message"].as_str())
                        .or_else(||event["response"]["incomplete_details"]["reason"].as_str())
                        .unwrap_or("unknown provider error")));
            }
            _=>{},
        }
        Ok(())
    }
    /// Convert only fully settled streams to the same provider-specific
    /// response structures expected by existing typed tool-call decoders.
    pub(crate) fn finish(self)->Result<Value,String>{
        if !self.done{return Err("Native stream ended before its terminal completion event".into());}
        match self.wire {
            Wire::Codex=>self.codex_response
                .ok_or("Codex missing settled response".into()),
            Wire::Chat=>{
                let finish=self.chat_finish.ok_or("Chat [DONE] without finish_reason")?;
                if finish=="stop" && !self.calls.is_empty(){
                    return Err("Chat emitted tool calls but declared text completion".into());
                }
                let mut calls=Vec::new();
                for (_,c) in self.calls {
                    if c.id.is_empty()||c.name.is_empty()||c.args.is_empty(){
                        return Err("Incomplete native tool call at Chat terminal event".into());
                    }
                    calls.push(json!({"id":c.id,"type":"function",
                        "function":{"name":c.name,"arguments":c.args}}));
                }
                if finish=="tool_calls" && calls.is_empty(){
                    return Err("Chat finished with tool_calls but provided none".into());
                }
                Ok(json!({"choices":[{"index":0,"finish_reason":finish,
                    "message":{"role":"assistant","content":self.text,
                        "tool_calls":calls}}],
                    "usage":{"prompt_tokens":self.prompt_tokens,
                        "completion_tokens":self.output_tokens,
                        "prompt_tokens_details":{"cached_tokens":self.cached_read_tokens}}}))
            }
            Wire::Anthropic=>{
                let mut blocks=Vec::new();
                for (_,item) in self.blocks {
                    match item {
                        Block::Text(text)=>blocks.push(json!({"type":"text","text":text})),
                        Block::Ignored=>{},
                        Block::Tool{id,name,partial,initial}=>{
                            let input=if partial.is_empty(){initial}else{
                                serde_json::from_str::<Value>(&partial)
                                    .map_err(|e|format!("Incomplete Anthropic tool JSON: {e}"))?
                            };
                            if !input.is_object(){
                                return Err("Anthropic tool arguments must be an object".into());
                            }
                            blocks.push(json!({"type":"tool_use","id":id,
                                "name":name,"input":input}));
                        }
                    }
                }
                Ok(json!({"type":"message","stop_reason":self.anthropic_stop,
                    "content":blocks,"usage":{"input_tokens":self.prompt_tokens,
                        "output_tokens":self.output_tokens,
                        "cache_read_input_tokens":self.cached_read_tokens,
                        "cache_creation_input_tokens":self.cached_write_tokens}}))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn encode(value:Value)->Vec<u8> {
        format!("data: {}\r\n\r\n",value).into_bytes()
    }
    #[test]
    fn native_chat_duplicate_finish_is_idempotent_until_done_and_preserves_usage() {
        // Cloudflare/OpenAI-compatible streams may send a repeated stop or
        // tool_calls marker before the final usage-only SSE and [DONE].
        for reason in ["stop","tool_calls"] {
            let mut a=Assembler::new(Wire::Chat);
            if reason=="tool_calls" {
                a.feed(Packet::Data(json!({"choices":[{"index":0,
                    "delta":{"tool_calls":[{"index":0,"id":"tool-1",
                        "function":{"name":"workspace_read",
                            "arguments":"{\\\"path\\\":\\\"a.txt\\\"}"}}]}}]}))).unwrap();
            }else{
                a.feed(Packet::Data(json!({"choices":[{"index":0,
                    "delta":{"content":"BENCH_OK"}}]}))).unwrap();
            }
            let terminal=json!({"choices":[{"index":0,"delta":{},
                "finish_reason":reason}]});
            let first=a.feed(Packet::Data(terminal.clone())).unwrap();
            let again=a.feed(Packet::Data(terminal)).unwrap();
            assert!(again.is_empty(),"duplicate terminal must not emit extra tool/text updates");
            if reason=="tool_calls" {
                assert_eq!(first.iter().filter(|u|matches!(u,Update::ToolStarted{..})).count(),1);
            }
            a.feed(Packet::Data(json!({"choices":[{"index":0,"delta":{},
                "finish_reason":reason}],"usage":{"prompt_tokens":35,
                    "completion_tokens":8,
                    "prompt_tokens_details":{"cached_tokens":21}}}))).unwrap();
            a.feed(Packet::Data(json!({"choices":[],"usage":{
                "prompt_tokens":35,"completion_tokens":8}}))).unwrap();
            // Completion is gated by the real [DONE] packet (checked below).
            a.feed(Packet::Done).unwrap();
            let result=a.finish().unwrap();
            assert_eq!(result["usage"]["prompt_tokens"],35);
            assert_eq!(result["usage"]["completion_tokens"],8);
            assert_eq!(result["usage"]["prompt_tokens_details"]["cached_tokens"],21);
            assert_eq!(result["choices"][0]["finish_reason"],reason);
            if reason=="stop" {
                assert_eq!(result["choices"][0]["message"]["content"],"BENCH_OK");
            }else{
                assert_eq!(result["choices"][0]["message"]["tool_calls"][0]["id"],"tool-1");
            }
        }
    }
    #[test]
    fn native_chat_rejects_changed_finish_and_post_finish_mutation() {
        for later in [
            json!({"choices":[{"finish_reason":"tool_calls","delta":{}}]}),
            json!({"choices":[{"delta":{"content":"additional text"}}]}),
            json!({"choices":[{"delta":{"tool_calls":[{"index":0,
                "function":{"arguments":"{}"}}]}}]}),
        ] {
            let mut a=Assembler::new(Wire::Chat);
            a.feed(Packet::Data(json!({"choices":[{"finish_reason":"stop",
                "delta":{}}]}))).unwrap();
            assert!(a.feed(Packet::Data(later)).is_err(),
                "a changed stop reason or trailing content must not be accepted");
        }
    }
    #[test]
    fn native_chat_duplicate_terminals_over_fragmented_sse_do_not_duplicate_content() {
        let mut wire=Vec::new();
        for item in [
            json!({"choices":[{"index":0,"delta":{"content":"한글 BENCH_OK"}}]}),
            json!({"choices":[{"index":0,"finish_reason":"stop","delta":{}}]}),
            json!({"choices":[{"index":0,"finish_reason":"stop","delta":{}}]}),
            json!({"choices":[],"usage":{"prompt_tokens":11,"completion_tokens":5}}),
        ]{wire.extend(encode(item));}
        wire.extend(b"data: [DONE]\\r\\n\\r\\n");
        let mut decoder=SseDecoder::default();
        let mut assembler=Assembler::new(Wire::Chat);
        let mut seen=String::new();
        for part in wire.chunks(3){
            for packet in decoder.push(part).unwrap(){
                for update in assembler.feed(packet).unwrap(){
                    if let Update::Text(delta)=update {seen.push_str(&delta)}
                }
            }
        }
        decoder.end().unwrap();
        let result=assembler.finish().unwrap();
        assert_eq!(seen,"한글 BENCH_OK");
        assert_eq!(result["choices"][0]["message"]["content"],"한글 BENCH_OK");
    }
    #[test]
    fn native_sse_preserves_reported_cache_tokens_from_chat_and_anthropic() {
        let mut chat=Assembler::new(Wire::Chat);
        chat.feed(Packet::Data(json!({"choices":[{"delta":{},
            "finish_reason":"stop"}],"usage":{
                "prompt_tokens":800,"completion_tokens":20,
                "prompt_tokens_details":{"cached_tokens":640}}}))).unwrap();
        chat.feed(Packet::Done).unwrap();
        let chat=chat.finish().unwrap();
        assert_eq!(chat["usage"]["prompt_tokens_details"]["cached_tokens"],640);
        let mut anthropic=Assembler::new(Wire::Anthropic);
        for event in [
            json!({"type":"message_start","message":{"usage":{
                "input_tokens":40,"cache_read_input_tokens":400,
                "cache_creation_input_tokens":100}}}),
            json!({"type":"content_block_start","index":0,
                "content_block":{"type":"text","text":"done"}}),
            json!({"type":"content_block_stop","index":0}),
            json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},
                "usage":{"output_tokens":15}}),
            json!({"type":"message_stop"}),
        ] {anthropic.feed(Packet::Data(event)).unwrap();}
        let anthropic=anthropic.finish().unwrap();
        assert_eq!(anthropic["usage"]["input_tokens"],40);
        assert_eq!(anthropic["usage"]["cache_read_input_tokens"],400);
        assert_eq!(anthropic["usage"]["cache_creation_input_tokens"],100);
        assert_eq!(anthropic["usage"]["output_tokens"],15);
    }

    #[test]
    fn native_sse_transport_splits_unicode_and_json() {
        let mut dec=SseDecoder::default();
        let payload=encode(json!({"choices":[{"index":0,"delta":{"content":"한글🧠"}}]}));
        let mut events=vec![];
        for byte in payload {events.extend(dec.push(&[byte]).unwrap());}
        assert_eq!(events.len(),1);
        let mut a=Assembler::new(Wire::Chat);
        let updates=a.feed(events.pop().unwrap()).unwrap();
        assert!(matches!(&updates[0],Update::Text(t) if t=="한글🧠"));
    }
    #[test]
    fn native_sse_interleaved_tool_calls_commit_only_after_done() {
        let mut a=Assembler::new(Wire::Chat);
        for event in [
            json!({"choices":[{"index":0,"delta":{"tool_calls":[
                {"index":1,"id":"idB","function":{"name":"workspace_read","arguments":"{\"p"}},
                {"index":0,"id":"idA","function":{"name":"workspace_read","arguments":"{\"p"}}
            ]}}]}),
            json!({"choices":[{"index":0,"delta":{"tool_calls":[
                {"index":0,"function":{"arguments":"ath\":\"a\"}"}},
                {"index":1,"function":{"arguments":"ath\":\"b\"}"}}
            ]}}]}),
            json!({"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}),
        ] {a.feed(Packet::Data(event)).unwrap();}
        assert!(a.finish().is_err(),"No tool is executable before terminal [DONE]");
        let mut a=Assembler::new(Wire::Chat);
        for event in [
            json!({"choices":[{"index":0,"delta":{"tool_calls":[
                {"index":1,"id":"idB","function":{"name":"workspace_read","arguments":"{\"p"}},
                {"index":0,"id":"idA","function":{"name":"workspace_read","arguments":"{\"p"}}
            ]}}]}),
            json!({"choices":[{"index":0,"delta":{"tool_calls":[
                {"index":0,"function":{"arguments":"ath\":\"a\"}"}},
                {"index":1,"function":{"arguments":"ath\":\"b\"}"}}
            ]}}]}),
            json!({"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}),
        ] {a.feed(Packet::Data(event)).unwrap();}
        a.feed(Packet::Done).unwrap();
        let v=a.finish().unwrap();
        assert_eq!(v["choices"][0]["message"]["tool_calls"][0]["id"],"idA");
        assert_eq!(v["choices"][0]["message"]["tool_calls"][1]["id"],"idB");
    }
    #[test]
    fn native_sse_anthropic_partial_json_and_settlement() {
        let mut a=Assembler::new(Wire::Anthropic);
        for item in [
            json!({"type":"message_start","message":{"usage":{"input_tokens":14}}}),
            json!({"type":"content_block_start","index":0,
                "content_block":{"type":"tool_use","id":"tool_1","name":"workspace_read","input":{}}}),
            json!({"type":"content_block_delta","index":0,
                "delta":{"type":"input_json_delta","partial_json":"{\"path\":\""}}),
            json!({"type":"content_block_delta","index":0,
                "delta":{"type":"input_json_delta","partial_json":"src/lib.rs\"}"}}),
            json!({"type":"content_block_stop","index":0}),
            json!({"type":"message_delta","delta":{"stop_reason":"tool_use"},
                "usage":{"output_tokens":9}}),
            json!({"type":"message_stop"}),
        ] {a.feed(Packet::Data(item)).unwrap();}
        let settled=a.finish().unwrap();
        assert_eq!(settled["content"][0]["input"]["path"],"src/lib.rs");
        assert_eq!(settled["usage"]["input_tokens"],14);
    }
    #[test]
    fn native_sse_codex_requires_terminal_full_response() {
        let mut a=Assembler::new(Wire::Codex);
        a.feed(Packet::Data(json!({"type":"response.output_item.done",
            "item":{"type":"function_call","call_id":"call_1",
                "name":"shell_exec","arguments":"{}"}}))).unwrap();
        assert!(a.finish().is_err());
        let mut a=Assembler::new(Wire::Codex);
        a.feed(Packet::Data(json!({"type":"response.completed",
            "response":{"status":"completed","output":[{"type":"function_call",
                "call_id":"call_1","name":"workspace_read","arguments":"{}"}]}}))).unwrap();
        assert_eq!(a.finish().unwrap()["output"][0]["call_id"],"call_1");
    }
    #[test]
    fn native_sse_anthropic_thinking_is_ignored_and_unclosed_tools_are_rejected() {
        let mut ok=Assembler::new(Wire::Anthropic);
        let events=[
            json!({"type":"message_start","message":{"usage":{"input_tokens":100}}}),
            json!({"type":"content_block_start","index":0,
                "content_block":{"type":"thinking","thinking":""}}),
            json!({"type":"content_block_delta","index":0,
                "delta":{"type":"thinking_delta","thinking":"private reasoning"}}),
            json!({"type":"content_block_stop","index":0}),
            json!({"type":"content_block_start","index":1,
                "content_block":{"type":"text","text":""}}),
            json!({"type":"content_block_delta","index":1,
                "delta":{"type":"text_delta","text":"Ready"}}),
            json!({"type":"content_block_stop","index":1}),
            json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},
                "usage":{"output_tokens":6}}),
            json!({"type":"message_stop"}),
        ];
        for item in events {ok.feed(Packet::Data(item)).unwrap();}
        let value=ok.finish().unwrap();
        assert_eq!(value["content"][0]["text"],"Ready");
        assert_eq!(value["content"].as_array().unwrap().len(),1);
        let mut incomplete=Assembler::new(Wire::Anthropic);
        incomplete.feed(Packet::Data(json!({"type":"content_block_start","index":0,
            "content_block":{"type":"tool_use","id":"tool_0","name":"shell_exec","input":{}}}))).unwrap();
        incomplete.feed(Packet::Data(json!({"type":"message_delta",
            "delta":{"stop_reason":"tool_use"}}))).unwrap();
        assert!(incomplete.feed(Packet::Data(json!({"type":"message_stop"}))).is_err(),
            "Unclosed partial tool calls may never be scheduled");
    }
    #[test]
    fn native_sse_fail_closed_on_errors_and_missing_terminal() {
        let mut d=SseDecoder::default();
        assert!(d.push(b"data: {\"type\":\"partial\"").unwrap().is_empty());
        assert!(d.end().is_err());
        let mut a=Assembler::new(Wire::Chat);
        a.feed(Packet::Data(json!({"choices":[{"index":0,
            "finish_reason":"length","delta":{"content":"partial"}}]}))).unwrap_err();
        let mut b=Assembler::new(Wire::Anthropic);
        assert!(b.feed(Packet::Data(json!({"type":"error",
            "error":{"message":"temporarily overloaded"}}))).is_err());
        let mut c=Assembler::new(Wire::Codex);
        assert!(c.feed(Packet::Data(json!({"type":"response.incomplete",
            "response":{"incomplete_details":{"reason":"max_output_tokens"}}}))).is_err());
    }
}