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
