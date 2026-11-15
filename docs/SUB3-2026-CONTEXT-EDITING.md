# 2026년 컨텍스트 편집 기반 실비용 절감 — 서브3

## 2026년 공개자료와 실제 외부 코드

- **2026-11-09**, Park et al., *System Optimizations for Cost-Efficient Agent Harnesses* (Agency), https://arxiv.org/abs/2611.12122 — 하네스 차원의 비용 개선은 성능·모델 변경 없이도 큰 영향을 줄 수 있다는 동기. 논문의 최대 35% 절감 수치를 DOMINION의 실측 성과로 사용하지 않는다.
- **2026-11 현재**, Anthropic 공식 Context Editing, https://platform.claude.com/docs/en/build-with-claude/context-editing — 오래된 tool-result만 제거, 최근 결과 보존, 최소 절감량 기준으로 프롬프트 캐시 무효화 비용 관리.
- **2026-11 현재**, NousResearch Hermes Agent, https://github.com/NousResearch/hermes-agent/blob/main/agent/context_compressor.py 및 https://github.com/NousResearch/hermes-agent/blob/main/website/docs/developer-guide/context-compression-and-caching.md — 실제 압축 게이트·컨텍스트 사용량 앵커·도구 결과 처리 경로를 참고. 코드를 복제하거나 별도 모델/외부 하네스를 도입하지 않는다.

## DOMINION 실제 구현 경로

`src/engine/chat/provider.rs`:
1. `stream_completion_once_body` (OpenAI 네이티브/호환) 및 `stream_anthropic_once` (Anthropic) → `fit_tool_context` → `edit_stale_read_context`.
2. 제공자에 `context_tokens`가 등록되어 있고 **입력 예산의 75% 이상**을 소모할 때만 대상 계산. 고정 system/user 지시와 assistant 호출·tool_call_id를 변경하지 않는다.
3. **최신 도구 결과 3개**, 오류·쓰기 및 상태 변경 결과는 원문 보존. 오래된 큰 `workspace_read`, `file_read`, `web_search` 등의 읽기 출력만 시작·끝 미리보기/실행 ID/원본 길이로 교체한다.
4. 예상 JSON 전송문자 절감량이 8천~2.4만 자 기준보다 작으면 변경하지 않는다. 공백·개행·한글도 Unicode 문자 경계로 안전하게 처리한다.
5. 저장된 `execution_operation`·원본 메시지는 그대로 유지하며, 각 후보 제공자의 전송 사본에서만 적용한다. 동일 tool_call_id가 다른 턴에서 다시 쓰여도 **해당 assistant 묶음에서 발생한 도구 유형**으로 분류하여 예전 쓰기 증거를 지우지 않는다.
6. 새 모델 호출·요약 API 비용은 0회. `chat.context_edit_prepared` 감사 이벤트에 `removedJsonCharsEstimate`를 남긴다. **실제 과금 토큰 절감은 별도 측정 전까지 null**로 기록한다.
7. Codex의 연속 WebSocket 체인은 별도 서버측 지속 문맥이므로 기존 경로를 유지한다. 등록된 컨텍스트 크기가 없는 제공자는 임의의 한계를 가정해 강제 압축하지 않는다.

## 검증

`src/engine/chat/provider.rs` 회귀 테스트:
- `cache_aware_context_editing_saves_old_reads_but_retains_effects_and_recent_results`: 최소 30,000개의 직렬화 문자 절감, 최근 3개·쓰기 증거·오류·호출 ID 보존, 원본 불변.
- `old_tool_error_and_write_receipt_are_never_context_edited`: 오류·쓰기 결과 제외.
- `reused_tool_ids_never_reclassify_an_earlier_write_as_a_read`: 반복된 도구 호출 ID를 이전 작업과 혼동하지 않음.
- `anthropic_native_stream_translates_messages_tools_and_usage`: 실제 mock HTTP 전송의 압축된 본문·최근 결과·쓰기·오류·원본 불변 및 감사 이벤트 확인.

실제 토큰 청구와 프롬프트 캐시 hit/miss는 모델·제공자별로 기존 `model.usage` 감사 기록을 비교해야 한다. 테스트에 나타난 입력 문자량의 감소를 비용 절감률로 환산해 주장하지 않는다.
