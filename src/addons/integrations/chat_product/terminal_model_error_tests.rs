use super::*;

    #[test]
    fn non_answer_with_usage_and_reasoning_only_is_not_success() {
        let empty=crate::engine::chat::Completion::default();
        assert!(terminal_model_error(&empty).unwrap().contains("읽을 수 있는 답변"));
        let mut reasoning=crate::engine::chat::Completion::default();
        reasoning.reasoning="private reasoning".into();
        assert!(terminal_model_error(&reasoning).unwrap().contains("최종 답변"));
        reasoning.content="Final answer".into();
        assert_eq!(terminal_model_error(&reasoning),None);
    }

    #[test]
    fn truncated_and_filtered_answers_are_failed_not_verified() {
        let mut completion=crate::engine::chat::Completion::default();
        completion.content="partial but visible".into();
        completion.finish_reason=Some("length".into());
        assert!(terminal_model_error(&completion).unwrap().contains("잘렸습니다"));
        completion.finish_reason=Some("content_filter".into());
        assert!(terminal_model_error(&completion).unwrap().contains("필터"));
        completion.finish_reason=Some("stop".into());
        assert_eq!(terminal_model_error(&completion),None);
    }

    #[test]
    fn tool_call_without_text_is_still_an_executable_model_result() {
        let mut completion=crate::engine::chat::Completion::default();
        completion.tool_calls.push(crate::engine::chat::ToolCall{
            id:"tool-1".into(), name:"workspace_read".into(), arguments:"{}".into()
        });
        completion.finish_reason=Some("tool_calls".into());
        assert_eq!(terminal_model_error(&completion),None);
    }
