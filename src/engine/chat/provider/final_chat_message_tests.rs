use super::*;

    #[test]
    fn final_message_only_restores_text_reasoning_and_native_calls() {
        let mut completion = Completion::default();
        let mut calls = Vec::new();
        let (text, reasoning) = merge_final_chat_message(
            &mut completion, &mut calls,
            &json!({"content":"안녕하세요","reasoning_content":"reason",
                "tool_calls":[{"id":"call-1","function":{
                    "name":"workspace_read","arguments":"{}"}}]}),
        ).unwrap();
        assert_eq!(text, "안녕하세요");
        assert_eq!(reasoning, "reason");
        assert_eq!(completion.content, "안녕하세요");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "call-1");
        assert_eq!(calls[0].name, "workspace_read");
    }

    #[test]
    fn final_snapshot_never_replays_accumulated_delta() {
        let mut completion = Completion::default();
        completion.content = "streamed".into();
        completion.reasoning = "thinking".into();
        let mut calls = vec![ToolCall {id:"id".into(),name:"shell_exec".into(),
            arguments:"{}".into()}];
        let (text, reasoning) = merge_final_chat_message(&mut completion, &mut calls,
            &json!({"content":"streamed","reasoning_content":"thinking",
                "tool_calls":[{"id":"other","function":{"name":"other","arguments":"{}"}}]}))
            .unwrap();
        assert!(text.is_empty());
        assert!(reasoning.is_empty());
        assert_eq!(completion.content, "streamed");
        assert_eq!(calls[0].id, "id");
    }

    #[test]
    fn full_final_snapshot_appends_only_the_missing_suffix() {
        let mut completion=Completion::default();
        completion.content="partial".into();
        completion.reasoning="think".into();
        let mut calls=Vec::new();
        let (text,reasoning)=merge_final_chat_message(
            &mut completion,&mut calls,
            &json!({"content":"partial text","reasoning_content":"thinking"})).unwrap();
        assert_eq!(text," text");
        assert_eq!(reasoning,"ing");
        assert_eq!(completion.content,"partial text");
        assert_eq!(completion.reasoning,"thinking");
        assert!(calls.is_empty());
    }

    #[test]
    fn usage_only_final_message_does_not_synthesize_an_answer() {
        let mut completion = Completion::default();
        let mut calls = Vec::new();
        let (text, reasoning) = merge_final_chat_message(&mut completion, &mut calls,
            &json!({"role":"assistant","content":null})).unwrap();
        assert!(text.is_empty() && reasoning.is_empty());
        assert!(completion.content.is_empty() && calls.is_empty());
    }

    #[test]
    fn final_snapshot_restores_second_tool_after_first_streamed() {
        let mut completion = Completion::default();
        let mut calls = vec![ToolCall {
            id: "call-1".into(), name: "workspace_read".into(),
            arguments: r#"{"path":"a.txt"}"#.into(),
        }];
        merge_final_chat_message(&mut completion, &mut calls, &json!({
            "tool_calls": [
                {"id":"call-1","function":{"name":"workspace_read","arguments":r#"{"path":"a.txt"}"#}},
                {"id":"call-2","function":{"name":"workspace_read","arguments":r#"{"path":"b.txt"}"#}}
            ]
        })).unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].id, "call-1");
        assert_eq!(calls[1].id, "call-2");
        assert_eq!(calls[1].arguments, r#"{"path":"b.txt"}"#);
    }

    #[test]
    fn final_snapshot_completes_partial_streamed_tool_arguments() {
        let mut completion = Completion::default();
        let mut calls = vec![ToolCall {
            id: "call-1".into(), name: "workspace_re".into(),
            arguments: r#"{"path":"#.into(),
        }];
        merge_final_chat_message(&mut completion, &mut calls, &json!({
            "tool_calls": [{
                "id":"call-1",
                "function":{"name":"workspace_read","arguments":r#"{"path":"a.txt"}"#}
            }]
        })).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "workspace_read");
        assert_eq!(calls[0].arguments, r#"{"path":"a.txt"}"#);
    }

    #[test]
    fn native_tool_index_gap_never_silently_drops_a_call(){
        let (calls,incomplete)=compat::finalize_streamed_tool_calls(vec![
            ToolCall{id:String::new(),name:String::new(),arguments:String::new()},
            ToolCall{id:"call-2".into(),name:"workspace_write".into(),
                arguments:r#"{"path":"proof.txt","content":"ok"}"#.into()},
        ]);
        assert!(incomplete,"index zero must not disappear");
        assert_eq!(calls.len(),2,"retain each streamed call for usage evidence");
        assert!(!calls[0].id.is_empty());
        assert_eq!(calls[1].id,"call-2");
    }

    #[test]
    fn complete_native_tool_calls_keep_ids_and_are_not_rejected(){
        let (calls,incomplete)=compat::finalize_streamed_tool_calls(vec![
            ToolCall{id:"call-1".into(),name:"workspace_read".into(),
                arguments:r#"{"path":"a.txt"}"#.into()},
            ToolCall{id:String::new(),name:"workspace_read".into(),
                arguments:r#"{"path":"b.txt"}"#.into()},
        ]);
        assert!(!incomplete);
        assert_eq!(calls[0].id,"call-1");
        assert!(!calls[1].id.is_empty());
    }
