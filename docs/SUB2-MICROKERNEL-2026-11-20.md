# 서브2 최소 코어 · 외부 서비스 아키텍처 (2026-11-20)

담당: dominion-sub2. 사용자 화면이나 API 경로가 아니라 실제 Rust 소스의 책임 영역을 재편합니다.

## Core
- src/engine/core/mod.rs : 자연어 요청을 기존 Rust 실행기로 전달하고 실행 결과·도구 영수증 수집
- src/engine/core/chat/runtime.rs : 요청→모델→도구→결과의 단일 실행 루프
- src/engine/core/chat/protocol.rs : 모델 응답과 도구 호출 해석
- 승인·효과(effect)·취소·결과 무결성은 실행 경계에 남겨 부가기능이 독립 실행 권한을 갖지 못하게 유지

## Services
- providers : 모델 선택, LLM 제공자, 스트리밍·전송, CLI 인증
- conversation : 메모리, 맥락 구성, 원문 조회, 멀티에이전트·방
- decision : 계획, 검증 보조, 판단, 평가, 프로그램 수리
- automation : 지속 작업, 스케줄·재개·복구
- tools : 파일·터미널·코드 검색 등 도구 구현 및 도구 탐색
- operations : 백업, 유지관리, 토큰 진단

## Addons
- MCP, 외부 계정과 GitHub/Google, 브라우저, Discord/Telegram/채널 메시징
- API 등록은 src/api/addons.rs 에서 맡으며, 기존 27개 URL 그대로 유지

## 바뀐 것과 안 바뀐 것
- engine 최상위의 60개 영역 모듈과 chat 내부의 제공자·맥락·탐색 12개 모듈을 파일 단위로 이동했습니다.
- 이동한 Rust 구현 파일은 Git blob SHA가 동일하며, 기존 모듈 이름은 Rust #[path] 선언으로 유지합니다.
- 기존 UI·메뉴, SDK 경로, DB 기록과 공개 HTTP 경로는 변경하지 않았습니다.
- 현재는 소스·책임 분리이며 Cargo 개별 패키지 분리나 기능 언로드를 구현했다고 주장하지 않습니다.
- 코어가 일부 서비스 함수를 직접 호출하는 기존 방식도 남아 있습니다. 포트 인터페이스 전면화를 아직 완료한 상태는 아닙니다.

## 검증
- scripts/sub2-core-boundary.test.mjs 에서 코어·서비스 파일 위치와 API 보존 점검
- scripts/sub2-addon-boundary.test.mjs 에서 MCP/연결 채널 격리 점검
- Rust CI에서 기존 engine::chat::* 경로로 실행 로직 재검증

2026년 아키텍처 참고:
https://github.com/QwenLM/qwen-code/blob/main/docs/developers/architecture.md
https://github.com/QwenLM/qwen-code/issues/11695

## 런타임 내부 추가 분리

ProgramIR 실행, 대안 후보 실험, 작업 검토, 정책 스냅샷과 세션 상태 관리 5개 파일도 `services/decision/` 및 `services/conversation/`으로 이동했습니다. 기존 단일 실행 루프는 결과를 수집하는 역할을 유지합니다.

## HTTP 경계 정리

- 코어 HTTP 주소는 자연어 실행·SDK 실행 제어·승인·이벤트·상태 확인 등 11개로 축소했습니다.
- 모델·대화·자동화·평가·운영의 기존 123개 API는 `src/api/surfaces/` 5개 라우터에서 등록합니다.
- MCP, OAuth, 브라우저, 연결 채널 등 27개 API는 기존 `src/api/addons.rs`가 등록합니다.
- 기존 핸들러 구현 27개 파일도 `src/api/services/` 또는 `src/api/addons/`로 위치를 옮겼습니다.
- 모든 라우터가 기존 Axum 서버의 공통 CORS·승인/보안·정비 중 차단 경계를 그대로 사용합니다.
- 새 메뉴, 새 API URL, 별도 HTTP 서버 또는 새 권한/저장소를 만들지 않았습니다.

2026-11-20 추가: 기존 코어의 그룹 대화 스트리밍/이벤트 전송은 `src/engine/services/conversation/room_worker.rs`로, 작업 재개는 `src/engine/services/automation/turn_resume.rs`로 이동했습니다. 기존 `engine::run_room_quiet`와 `engine::run_quiet_resume` 공개 경로는 재내보내기로 보존합니다.

## 실제 의존성 역전 (2026-11-20)

`core::ExecutionPort`는 자연어 실행 요청을 전달하는 제네릭 Rust 포트입니다. `core::run_with_events`는 서비스에서 발생한 도구 성공/실패/결과 불명 영수증을 수집하고 최종 결과를 그대로 반환합니다. `src/engine/services/automation/turn_quiet.rs`가 현재 `ChatPort`를 구현해 기존 채팅 서비스를 연결합니다. 이 경계 덕분에 코어는 `AppState`, 구체 모델 제공자, 메모리, 방, 계획, MCP를 직접 import하지 않습니다. `core::driver::Driver`는 실제 모델 호출을 provider Future 포트로 실행하고 도구 이름을 현재 모델에 제공된 스키마로 검사합니다. 독립적인 FakePort 회귀 테스트로 DB 없이 성공/실패 영수증 보존을 검증합니다.

## 독립 Cargo 경계

`crates/dominion-turn-core/`가 독립 Rust 크레이트로 등록되었습니다. 런타임 의존성은 `tokio`와 `serde_json`이며 provider·DB·UI·MCP·자동화 크레이트는 직접 가져오지 않습니다. 기존 제품 API는 `src/engine/core/mod.rs` 단일 facade가 재내보냅니다. `cargo test --locked -p dominion-turn-core --lib`를 CI에 추가해 호스트 포트를 실제 DB·모델 없이 검증합니다.

## 2026-11-20: 알고리즘 ↔ 하네스 종속성 역전

- **불변 실행 커널**: `crates/dominion-turn-core/` — 모델 라운드 수·도구 광고 범위·중단/실행 영수증; 현재 모델 제공자, 계획 알고리즘, 메모리, 저장소, MCP를 import하지 않습니다.
- **교체 가능한 순수 알고리즘**: `crates/dominion-agent-kernel/src/decision_policy.rs` — 계획 수립 승인, 도구 실행 순서, 읽기 조합, 읽기 선행 정책, 재시도 전략.
- **알고리즘 선택 어댑터**: `src/engine/services/decision/turn_policy.rs` — 기존 SQLite 활성 알고리즘과 정책 slot을 조회하고 타입이 지정된 결정을 반환합니다.
- **제품 서비스**: `src/engine/services/conversation/chat/runtime/turn_body.rs` — 직접 `algorithms::choose_for_agent`나 `policy::active`를 호출하지 않고 어댑터를 사용합니다. 기존 모델·도구 호출, 승인, 환경 fingerprint, 예산, 영수증 로직을 보존합니다.

기존 활성 `implementation` 값(`read-first`, `parallel-reads`, `block-write-without-read`, `allow-all` 등)을 그대로 해석합니다. 미등록 이름은 기존 기본 동작으로 회귀하며 커널 권한이 확대되지 않습니다. `tools_enabled`, 사용자 승인, 샌드박스, 실제 도구 실행은 해당 알고리즘이 우회할 수 없습니다.

**구현 범위**: 이번 단계는 계획·행동 선택·복구 정책을 커널에서 격리한 것입니다. 복합 채팅 실행 오케스트레이션 자체를 별도 런타임 프로세스로 완전히 재구축하거나, 실행 도중 동적으로 Rust 코드를 로드하는 방식은 아닙니다. 프론트 메뉴/API/메인은 그대로 유지합니다.

참고: [OpenClaw 공식 런타임 구조](https://docs.openclaw.ai/agent-runtime-architecture), [런타임 플러그인](https://docs.openclaw.ai/concepts/agent-runtimes).

## 독립 AgentLoop Trait / Rust Workspace 실행 알고리즘

2026-11-20: 계약 `crates/dominion-turn-core/src/agent_loop.rs`에 `AgentLoop`, `ModelPort`, `ToolPort`, `RoundObservation`을 정의했습니다. **알고리즘은 모델의 도구 호출을 추가하거나 무단 실행시킬 수 없습니다**: 최종 상태 전이는 코어 `Driver::authorize_step`가 검증합니다. 제품의 현재 채팅 실행 루프도 `TurnPolicy::workflow`로 선택된 알고리즘을 통과합니다.

독립 크레이트 `crates/dominion-agent-loop/`는 `ReActLoop`와 `PlanExecuteLoop`를 구현하고, 실제 모델→도구→모델 반복을 수행할 수 있는 범용 `run` 함수를 제공합니다. 모의 제공자·도구를 활용해 실제 라운드, 툴 광고 경계, 취소, 모델 라운드 상한 회귀 테스트를 제공합니다. 기존 UI, 엔드포인트, 저장소, 구체 모델 및 도구 코드는 종전 서비스 계층에서만 관리합니다.

**실행 범위**: 현재 제품의 기존 대규모 `chat/runtime/turn_body.rs`는 실행 세션 영속화·복합 증거 수집 등을 위해 유지되고, 새 에이전트 알고리즘이 각 모델 응답의 실행 상태 전이에서 실사용됩니다. 새 `run` 전체를 아직 기존 제품의 모든 실행 경로로 대체한 것은 아닙니다.

참고: https://docs.openclaw.ai/agent-loop, https://github.com/zeroclaw-labs/zeroclaw/blob/master/docs/book/src/architecture/overview.md, https://github.com/nearai/ironclaw .

## 알고리즘 버전 선택 (UI 변경 없음)

기존 정책 저장소의 활성 `agent.loop` 슬롯에서 `parameters.implementation` 값 `react` 또는 `plan-execute`를 읽어 작업별로 루프 구현을 선택합니다. 저장된 선택이 없거나 알 수 없는 이름이면 기존 작업 기반 자동 선택을 사용합니다. 이 레지스트리는 현재 컴파일된 알고리즘의 선택 방식이며, 임의 Rust/WASM 코드를 런타임에 동적으로 로드하는 것은 아닙니다. 외부 모델 인증·승인·샌드박스·도구 실행 허용은 실행 코어 및 호스트에 남습니다.

## 2026-11-20 그룹 실행 공유 작업판 (Blackboard)

기존 rooms.rs의 체크포인트와 세션별 직렬화는 유지합니다. 도구 실행이 켜져 있고 참가자가 2명 이상인 그룹 작업에서 실제로 저장된 assistant 답변을 sourceMessageId(불변 주인 원문)별 `room_work_item`으로 추가합니다. 메모리 전체 스캔 대신 SQLite expression index `(roomId,sourceMessageId)`를 사용합니다. 재개나 재시도 시 `roomId/sourceMessageId/messageId`에서 유도한 결정적 ID와 `put_if_absent`로 중복 기록을 방지합니다.

다음 참가자 문맥에는 최대 6개의 짧은 관찰과 완료된 실제 `execution_operation` ID/도구/리소스 참조만 넣습니다. 모델 주장과 검증된 도구 관측을 구분하며 진행 중/결과 불명 효과는 근거로 승격하지 않습니다. 과거/미래 소유자 메시지, 타 대화방, 미저장 메시지, 에이전트 개인 프롬프트는 전파하지 않습니다. 구조화된 작업판은 모델과 하네스 독립적인 제품 서비스이며 기존 UI 및 API를 수정하지 않습니다.

근거 비교: https://docs.openclaw.ai/concepts/queue (세션 lane 직렬화), https://docs.rs/adk-rust/latest/adk_rust/agent/team/index.html (공유 블랙보드), https://github.com/whatstask/Cairn (durable event provenance).

## 그룹 병렬 실행 — 소유자 옵트인, 읽기 전용 격리

그룹 JSON 설정 `executionMode=parallel_readonly`는 **단일 라운드·최대 8명·실제 독립 참가자 2명 이상**이고 도구 실행이 꺼진 경우에만 작동합니다. 모델 호출은 `tokio::task::JoinSet`에서 최대 4명씩 격리된 내부 `room_worker` 세션에서 병렬 처리하며, 완료 뒤 참가자 순서대로 공용 대화 메시지를 영속화합니다. 기존 그룹 토론, 도구 승인, 쓰기 작업, 다중 라운드는 이전 순차 실행 경로를 유지합니다. 신규 메뉴·UI는 추가하지 않고 기존 대화방 설정 API에서만 선택 가능합니다. 내부 세션은 사용자 세션 목록에서 숨겨지고, 이력·원문·모델 응답은 감사 추적을 위해 저장됩니다. 사용자가 새 요청을 보내거나 취소하면 해당 작업의 워커를 중단하며, 불확실한 충돌 후에는 기존 `inFlight`/검토 필요 정책을 유지합니다. 공유 작업판은 병렬 완료를 다시 참조하되 사용자 요청별 범위에서만 읽습니다.

## Context middleware 독립 Rust 크레이트

`crates/dominion-context-policy`는 기존 `chat_trace_budget.rs`의 provider-wire 압축 알고리즘을 실제 독립 크레이트로 옮긴 것입니다. `bounded_tool_trace`와 `compact_historical` 및 7개의 단위 회귀 테스트를 보존했습니다. `serde_json` 외부 런타임 의존성이 없으며 SQLite 원본, 도구 영수증, 주인 메시지 내용을 변경하지 않습니다. 완료된 도구 호출+실제 응답을 프로토콜 원자 단위로 다루고 모델에만 짧은 비명령형 요약을 전달하며, 현재 source read가 필요한 상황에서는 원본을 유지합니다. 기존 chat::trace_budget 서비스는 호환 래퍼만 남겨 UI/API 변경 없이 실행 경로에 적용합니다.

## 모델별·엔드포인트별 Capability Admission

`gateway::ModelRequirements`는 `text`, `toolCalling`, `vision`, `reasoningSupported` 등의 명시적 거부를 우선 적용합니다. `provider.modelCapabilities[modelId]`가 있으면 제공자 기본 `capabilities`보다 먼저 적용하며, 대체 모델은 사전에 현재 도구 스키마·이미지 요구사항과 비교합니다. 기존 미등록 제공자/모델은 보수적인 호환 동작을 유지하되 **명시적으로 지원하지 않는 기능을 fallback에서 실행하지 않습니다.** Codex/Claude 등 외부 에이전트 실행기 연결은 이 ModelGateway와 별도 AgentBridge 계약이 필요합니다.

## 공통 실행 상태와 작업 재실행 정합성

독립 코어에 RunState/RunEvent 버전 1 계약을 추가하고, 그룹 작업 할당과 완료/건너뜀 전이에 연결했습니다. 작업 ID는 주인 원문, 에이전트, 작업 순서에서 결정적으로 유도되어 작업 재개에서 중복 할당을 만들지 않습니다. Chat/SDK 전체 이벤트의 전환까지 완료한 것은 아닙니다.

## 2026-11-20: 도구 배치 원자적 승인과 실패 영수증

독립 Driver::admit_batch는 모델이 실제로 제안한 도구 호출 **전체**를 실행 전에 확인합니다.
광고되지 않은 이름, 같은 턴에서 재사용된 호출 ID, 중복/빈 ID, 남은 예산 초과를
배치 단위로 거부합니다. 거부된 배치에서 도구가 일부라도 먼저 실행되는 일을 막습니다.

독립 dominion-agent-loop::run은 이 원자적 승인을 사용합니다.
도구 호출이 시작된 뒤 전송 오류가 나면 호스트 효과를 확정할 수 없으므로
해당 영수증에는 outcomeUnknown=true를 기록하고 자동 재실행 없이 종료합니다.
남은 호출은 not_dispatched로 기록하며 assistant tool_calls와 tool result의
프로토콜 짝을 보존합니다. 취소된 후속 호출도 not_dispatched로 보존합니다.
실제 제품의 기존 복합 채팅 루프/영속 저장소는 이번 단계에서 변경하지 않았습니다.

비교한 2026 공개 구현:
- Qwen Code (2026-07-20): https://github.com/QwenLM/qwen-code/blob/main/docs/design/2026-07-20-plan-mode-mid-turn-guidance.md
- OpenClaw agent loop: https://docs.openclaw.ai/agent-loop
- Qwen Code tool telemetry: https://github.com/QwenLM/qwen-code/blob/main/docs/developers/development/telemetry.md

## 2026-11-20: Chat·Room·SDK 공통 RunState 이벤트 경계

- 기존 `Bus::record` 경계에서 `job`(SDK/automation), `execution`(채팅), `room_work_assignment`(그룹 작업)의 **저장된** 상태를 `dominion-turn-core::run_state::RunEvent`로 투영합니다.
- 모든 클라이언트는 기존 record 이벤트와 별도로 `type=run_state`, `schemaVersion=1`, `runKind`, `runId`, `state`, `eventSeq`를 받습니다. `eventSeq`는 SQLite 이벤트 저널의 전역 순서와 동일합니다. 한 변경에서 파생된 record/run_state 2개가 끼어들지 않도록 같은 락으로 기록/방송합니다.
- owner 원문 ID가 없으면 `sourceMessageId=null`을 내보냅니다. SDK job ID를 owner 메시지라고 날조하지 않습니다.
- 기존 그룹 할당/완료/건너뜀 이벤트의 인위적 step×2 계산과 중복 수동 발행을 제거했습니다. 성공한 CAS 전이에서만 실제 run_state를 방송하며, 완료 재생은 새 이벤트를 발생시키지 않습니다.
- 이 변경은 **단일 이벤트 스키마의 실제 소비 가능 경계**입니다. 기존 복합 Chat 실행 루프와 독립 AgentLoop 통합을 완료했다는 뜻은 아닙니다. 저장된 실행 내용, API URL, UI, 메인 브랜치의 기능은 변경하지 않았습니다.
- 2026 참고: https://openai.com/index/unlocking-the-codex-harness/ (안정적인 클라이언트용 실행 이벤트), https://github.com/QwenLM/qwen-code/blob/main/docs/developers/architecture.md (공통 에이전트 런타임 및 클라이언트 경계).

## 2026-11-20: 실제 제품 Chat ↔ 독립 AgentLoop 도구 배치 승인 통합

실제 Chat 실행기와 독립 `dominion-agent-loop::run`가 **동일한
`dominion-turn-core::Driver::admit_batch`**를 사용합니다. 이전 Chat의
호출별 승인에서는 앞선 도구가 실행된 후 뒤쪽의 숨겨진 MCP 호출을 발견할
수 있었으나, 이제 한 모델 응답 전체의 호출 ID·도구 광고 범위·예산을
**도구 실행 전에 일괄 검증**합니다.

`BudgetLedger::charge_proposals(count)`로 제안 예산을 원자적으로 예약하며,
거부된 배치에는 각 호출별 `dispatched:false` 오류 관찰을 남겨 모델이
다음 라운드에서 수정할 수 있게 합니다. 기존 개별 `driver.admit_tool`,
병렬 그룹 재광고 검사, 반복 `budget.charge_proposal` 경로를 실제
채팅 모델 응답 경로에서 제거했습니다. 승인·샌드박스·영속 효과 영수증,
연결된 MCP/도구 서비스는 기존 Host 소유입니다.

현재 통합 범위는 **모델 라운드·전략 판단·도구 배치 승인**이며, 독립
`dominion-agent-loop::run`가 기존 Chat의 영속 저장/복합 프로그램
진행과 완전히 교체됐다는 뜻은 아닙니다. UI·API URL·메인 브랜치는
변경하지 않았습니다.

2026 공개 설계 참고: Qwen Code managed agent actions (2026-09-30)
https://github.com/QwenLM/qwen-code/blob/main/docs/design/2026-09-30-managed-agent-actions.md
OpenAI Codex Agent Loop (2026-01-23)
https://openai.com/index/unrolling-the-codex-agent-loop/

## 2026-11-20: 도구 결과 메시지 공통화 및 재개 정합성

`dominion-turn-core::agent_loop::ToolFeedback`가 독립 AgentLoop와 실제
제품 채팅 실행기의 도구 결과 메시지 형식을 공통 소유합니다. 호스트 도구
결과 문자열은 변경하지 않고, 실패·취소로 실행하지 못한 호출은
`status=not_dispatched,dispatched=false`, 외부 효과를 확정할 수 없는
경우는 `outcomeUnknown=true`로 표시합니다.

기존 Chat의 소프트 거절(`batchRejected`, `procedureViolation`,
`toolSelectionBlocked`, 반복 실패, 잘못된 JSON 인수)은 **실제 모델에
전달한 동일한 JSON 결과를 SQLite tool 메시지에도 저장**합니다.
이전에는 모델에게 보이는 상세 사유가 저장 과정에서 `{"error":...}`로
축약되어 재개 뒤 인과관계가 바뀌는 문제가 있었습니다. 이벤트 수집기에
전달하는 `tool_error.result`도 동일하며 `dispatched=false`를
함께 보존합니다.

`soft_denial_replay_is_byte_identical_to_provider_transcript` 테스트는
일시적 모델 문맥과 재개용 SQLite 문맥이 바이트 단위로 같은
`content`를 갖는지 검증합니다. 기존 화면·엔드포인트·DB 스키마,
외부 도구 승인 및 사용자 데이터 위치는 변경하지 않았습니다.
단일 공통 Harness 통합은 **도구 결과 프레임 계층**까지 진전됐고,
모든 복합 프로그램의 독립 AgentLoop 이관은 아직 남아 있습니다.

## 2026-11-20: Harness 실행 단계/완료/그룹/SDK 경계 강화

- `run_state`의 순수 phase reducer를 `dominion-turn-core`로 통합했습니다. 제품 실행 단계 어댑터의 `apply`는 더 이상 `get + put`으로 중간 상태를 덮어쓰지 않고 SQLite IMMEDIATE CAS(`patch_if`)로 전이합니다. 상태 경쟁 시 최신 phase를 다시 읽고 검증하며 terminal run은 되살리지 않습니다.
- 최종 assistant 응답과 `execution.finalMessageId`를 **단일 SQLite 트랜잭션**에서 커밋하는 `Store::write_final_message_and_link`를 실제 `chat/runtime/finalize.rs`에 연결했습니다. 다른 세션의 응답, 이미 완료된 응답, 종료된 실행에 새로운 최종 메시지가 남지 않습니다.
- 그룹 작업 완료에서 모델이 보낸 근거 JSON을 곧바로 신뢰하지 않고, `room_work_item`에 이미 저장된 관측 메시지의 실제 소유·세션·태스크·메시지·증거 수를 검증합니다. 같은 범위로 위조한 결과도 거부합니다. 병렬 그룹 응답 공개 중 새 owner/중단 이벤트가 도착하면 남은 과거 답변을 더 공개하지 않습니다.
- Rust SDK `run_events_after`가 TypeScript `runEvents`와 같은 버전 1 `run_state` 이벤트를 원래 저널의 전역 cursor와 함께 제공합니다. HTTP/UI에 새 메뉴나 새 엔드포인트를 추가하지 않았습니다.
- 회귀 테스트는 CAS terminal monotonicity, SQLite final reply exactly-once, forged group observation rejection, Rust SDK journal parity를 포함합니다. 한 프로세스가 종료되는 전체 E2E와 100턴 end-to-end 모델 비용 실험은 별도 필요합니다.
- 참고: <https://openai.com/index/unlocking-the-codex-harness/> (thread/turn/item lifecycle), <https://github.com/QwenLM/qwen-code/blob/main/docs/design/2026-09-27-hosted-workspace-tool-turn.md> (원자적 도구 결과 지속과 재개).

## 2026-11-20: 크래시 재시작 및 100턴 그룹 컨텍스트

- `jobs::recover`가 부모 job을 queued로 되돌리기 전에 **running execution과 일치하는 최종 assistant 메시지**를 SQLite에서 검증합니다. 최종 메시지와 그 실행 링크가 이미 커밋된 경우에는 다시 모델 호출·외부 효과를 시작하지 않고 `needs_review`로 보류합니다. 이는 성공을 날조하지 않으면서 중복 비용/효과를 방지하는 복구 경계입니다.
- 그룹 작업판의 조회는 기존 SQLite 인덱스와 최근 48개 상한을 유지하면서, 기록이 48개를 넘으면 **최초 커밋된 관측 1개와 최신 47개**를 보존합니다. 모델로 전송되는 6개 요약은 최초 관측 1개와 최신 5개로 제한하여 오래된 그룹 합의/근거를 모두 잃지 않도록 합니다.
- 테스트: 저장된 최종 답변이 있던 job의 재시작 중복 실행 금지, 105턴 모델-프리 그룹 관측의 최초/최근 메시지 유지 및 중간 메시지 제외. 전체 실제 모델 100턴의 비용/지능 평가는 이 테스트의 검증 범위 밖입니다.

## 2026-11-20: 최종 완료 판정·결정적 실행도 단일 Harness 규칙

- `dominion-turn-core::run_state::terminal_result`를 제품 `finalize_execution`과 연결했습니다. 반환이 `Ok(None)`인 빈 최종 응답은 성공이 아닌 실패이며, 승인된 취소는 `cancelled`로 정규화합니다. `runtime`은 실제 사용자 결과도 빈 성공으로 돌려주지 않습니다.
- 모델 없는 정확 치환 경로(`run_frozen_replacements`)도 일반 Chat과 같은 `write_final_message_and_link`를 사용합니다. 기존의 최종 메시지 저장→실행 링크 별도 저장을 제거했습니다.
- 해시 검증된 deterministic read/write 단계 사이와 종료 전마다 동일한 session cancellation generation과 요청 취소를 검사합니다. 중단 후 남은 새 도구 호출이나 성공 답변을 만들지 않습니다.
- 여전히 하나의 거대한 AgentLoop 루프로 전체 코드가 완전히 물리적으로 합쳐진 것은 아니고, **도구·상태·최종 커밋 계약**이 일반 Chat·Model-free 경로·SDK·Room에서 재사용되는 구조입니다.

## 2026-11-20: 모든 제품 실행 이벤트의 원문 소유권 추적

일반 Chat에서는 최종 사용자 메시지를 영속 기록한 뒤 `preflight::bind_execution_source`가 실행 레코드의 `sourceMessageId`를 SQLite CAS로 연결합니다. 다른 원문으로 바꿔치기하는 재개는 거부하고 같은 원문을 반복 기록하지 않습니다. 그룹 작업은 이미 알고 있는 owner source를 execution 생성 시점부터 바인딩하여 `run_state` 이벤트가 올바른 소유자를 가리킵니다.

이에 따라 TypeScript `runEvents()`/Rust `run_events_after()`는 작업 ID만 보여주는 상태 스트림이 아니라, 가능한 실행에서 실제 커밋된 사용자 원문 메시지 ID와 연결되는 버전 1 이벤트입니다. UI 메뉴, HTTP URL, 저장소 브랜치 권한은 변경하지 않았습니다.

## 2026-11-21: 호환 껍데기 및 구형 우회 도구 승인 API 제거

- 서브2에서 실행 로직이 없는 `src/engine/core/mod.rs`, `src/engine/services/decision/agent_kernel.rs`, `src/engine/services/conversation/chat/budget.rs`, `src/engine/services/operations/chat_trace_budget.rs` 4개 파일을 **물리 삭제**했습니다. 기존 경로를 무조건 유지하는 얇은 shim 대신 `engine/mod.rs`와 `chat/mod.rs`가 각 독립 Rust crate를 직접 alias합니다. Rust API에서 사용 중인 `engine::core`, `engine::agent_kernel`, `chat::budget`, `chat::trace_budget` 이름은 유지합니다.
- `dominion-turn-core::Driver`에서 과거 **호출별 도구 허용** `admit_tool`과 **광고만 검사**하는 `batch_advertised` API를 삭제했습니다. 두 진입점은 전체 모델 응답의 ID·광고 범위를 원자적으로 검증하는 `admit_batch`보다 약했고, 실제 제품 경로는 이미 새 API를 사용하고 있었습니다.
- 기존 단위 테스트를 실패한 배치 후 ID를 재사용할 수 있는지, 숨겨진 도구가 포함된 응답이 먼저 실행되지 않는지 검증하는 직접 코어 테스트로 변경했습니다. 구조 검사도 삭제된 shim 복귀를 CI에서 차단하도록 전환했습니다.
- UI, HTTP 엔드포인트, 로컬/클라우드 모델 경로 및 다른 브랜치는 수정하지 않았습니다. 호출되는 실험/메모리/정책 모듈은 이 정리에서 임의 삭제하지 않았습니다.
