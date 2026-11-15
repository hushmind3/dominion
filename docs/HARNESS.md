# Dominion Rust SDK / Autonomous Runtime (2026-11-15)

**실제 소스 진입점**: `crates/harness/src/sdk.rs` (공개 Rust 라이브러리), `crates/harness/src/bin/dominion-harness.rs` (CLI), `src/api/sdk.rs` (제품 API), `web/src/components/NativeAgentPanel.tsx` (홈 화면). **주 엔진과 SDK는 Rust**이며 TypeScript/React는 API 클라이언트 UI입니다. 기존에 별도로 존재하던 `Harness::run` 루프는 미사용 중복 코드로 확인되어 제거했으며, 라이브러리의 공개 실행기는 `sdk::AgentSdk` 한 경로입니다.

## 실제 실행 경로

1. GUI 홈 화면 또는 외부 HTTP 클라이언트가 `POST /api/sdk/runs`에 사용자 작업 지시, `providerId`, 검증 명령(선택)을 전달합니다.
2. `src/api/sdk.rs`에서 Dominion의 기존 모델 설정을 `engine::gateway::resolve`로 읽습니다. OpenAI-호환 제공자에서만 현재 Rust SDK 실행을 시작합니다. 별도 에이전트/외부 SDK의 하네스는 호출하지 않습니다.
3. `sdk::AgentSdk`가 모델 호출 → 요청된 도구 확인 → `Policy` 결정 → 실행 → 모델에 정확한 도구 호출 ID와 결과를 제공하는 반복 루프를 소유합니다. `Provider`, `Tool`, `Policy`, `Verifier`를 모두 Rust 트레이트로 교체할 수 있습니다.
4. 쓰기(`fs_write`, `fs_patch`)·명령 실행(`shell`)은 제품 API에서 도구 호출 ID별 승인이 필요합니다. 읽기(`fs_read`, `fs_list`)는 허용합니다. GUI는 승인 전에 호출 인자를 보여줍니다.
5. `FileJournal`이 라운드, 도구 호출, 대기열, 메시지, 실행 이벤트를 `runtime/sdk-runs/{id}.json`에 저장합니다(`DOMINION_DATA` 사용 시 해당 디렉터리). 실행 직전에 `in_flight`를 영속화하며, 중단된 부작용은 자동 재실행하지 않습니다. 완료된 도구 호출 ID와 인자·응답도 기록하여 모델의 중복 호출이 재실행되지 않게 합니다. 사용자가 실제 상태를 확인해 `reconcile` API로 기록한 뒤 재개할 수 있습니다.
6. 검증 명령이 있으면 Rust `CommandVerifier`가 실제 워크스페이스에서 실행합니다. 검증 실패는 모델에 결과를 되돌려 수정/재검증하게 합니다. 검증기가 없는 경우에도 파일 작성·수정·실행처럼 부수 효과를 요구한 작업은 성공한 비읽기 도구 호출이 하나 이상 기록돼야 `completed`가 됩니다. 해당 기록 없이 모델이 완료를 주장하면 1회 수정 기회를 주고, 여전히 실제 실행 증거가 없으면 `exhausted(unverified_no_effect)`로 종료합니다. 일반 설명형 작업은 이 제한을 적용하지 않습니다. 별도 검증기 없이 완료한 경우 `independentlyVerified=false`로 명시하며, 성공한 도구 호출만으로 산출물의 정확성이나 테스트 통과를 주장하지 않습니다. 제공자가 실제 보고한 입력·출력·캐시 토큰만 누적하고 보고되지 않은 경우 사용량을 미표시합니다.

## 공개 API (제품)

| Method | Route | 용도 |
|---|---|---|
| POST | `/api/sdk/runs` | 작업 생성(`instruction`, `providerId?`, `model?`, `verifyCommand?`, `maxRounds?`) |
| GET | `/api/sdk/runs` | 최신 30개 작업 |
| GET | `/api/sdk/runs/:id` | 상태/이벤트/대기 중 도구 인자/최종 답 |
| POST | `/api/sdk/runs/:id/approve` | `{"callId":"..."}` 해당 도구 호출 1회 승인 |
| POST | `/api/sdk/runs/:id/cancel` | 작업 중단 |
| POST | `/api/sdk/runs/:id/reconcile` | `{"applied":true,"details":"실제 확인 결과"}` 중단된 도구 효과 관찰 기록 |
| POST | `/api/sdk/runs/:id/resume` | 기록에서 재개 |

## Rust CLI

```bash
cargo build -p dominion-harness --bin dominion-harness
DOMINION_HARNESS_KEY=YOUR_KEY cargo run -p dominion-harness --bin dominion-harness -- \
  --base-url https://example-openai-compatible-provider/v1 \
  --model selected-model \
  --task "프로젝트의 테스트 실패를 해결해줘" --workdir . \
  --allow-write --allow-shell --verify-command "cargo test" \
  --checkpoint ./runtime/agent-run.json
```

중단 후 `--resume --checkpoint ./runtime/agent-run.json`으로 실행합니다. `--approve-call ID`는 승인 대기 중인 1개 호출만 승인합니다. 중단 시 도구의 적용 여부가 불확실하면 파일·명령 결과를 실제 확인한 후 `--resume --checkpoint FILE --reconcile-details "확인한 결과" --reconcile-applied`로 확인 결과를 기록할 수 있습니다. 적용되지 않았을 경우 `--reconcile-applied`를 생략합니다. CLI는 제품 API를 거치지 않고 같은 `sdk::AgentSdk`를 직접 사용합니다.

## 2026 프런티어 비교와 아직 남은 통합

- **Codex**: 모델 턴과 ToolRouter, 컨텍스트/압축, 다중 실행 및 샌드박스 분리. 참고: [codex-rs/core/src/session/turn.rs](https://github.com/openai/codex/blob/main/codex-rs/core/src/session/turn.rs).
- **Qwen Code**: 중복 도구 호출 식별/재전송 방지, 도구 배치, 호스트 루프 분리. 참고: [agent-loop.ts](https://github.com/QwenLM/qwen-code/blob/main/packages/core/src/core/agent-loop.ts).
- **DeepSeek Harness**: core/loop, provider, 승인, subagent, sandbox, MCP를 교체형 서비스 경계로 구분. 참고: [packages/README.md](https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/README.md).
- **Hermes Agent 2026-11-05**: 하나의 게이트웨이가 세션을 소유하고 다양한 UI를 클라이언트로 연결. 참고: [v0.23.0 릴리스](https://github.com/NousResearch/hermes-agent/releases).

**현재 한계**: 신규 Rust SDK 작업 경로는 앱 홈과 API에 연결했으나 기존 `/api/chat`, `rooms`, `video`, `RSI`는 각각 이전 Rust 실행 흐름을 사용합니다. 이들을 신형 SDK로 완전 이전했다고 주장하지 않습니다. 현재 SDK 제공자 어댑터는 OpenAI 호환 채팅 완성 프로토콜 중심이고, 모델별 캐시 사용량/비용 계측·MCP/스킬·하위 에이전트·계획 DAG·병렬 도구·프로세스/네트워크 OS 샌드박스·완전한 Windows 원자적 체크포인트는 추가 구현 대상입니다. 쓰기 정책과 작업 디렉터리 제한은 OS 샌드박스가 아닙니다. 본격 SWE-bench Verified 동일 조건 비교는 핵심 기능을 통합한 후 진행합니다.

## 검증

추가한 단위 테스트는 `crates/harness/src/{lib,openai,context,tools,sdk}.rs`에 위치합니다. **이번 작업 세션은 Rust 컴파일러가 없는 환경이라 로컬 빌드/단위 테스트는 실행하지 못했습니다.** 모든 작업 경로에 대해 컴파일 및 종단 테스트 확인이 필요합니다. 잔재 정리와 실행 경로 검증을 완료하기 전에는 CI 완주를 성공 근거로 사용하지 않습니다.
