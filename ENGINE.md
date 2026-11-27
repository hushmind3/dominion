# DOMINION sub2 — 실행 엔진·아키텍처 (2026-11-21)

> **구현 정본:** [docs/SUB2-ARCHITECTURE-2026-11-21.md](docs/SUB2-ARCHITECTURE-2026-11-21.md). 이 파일은 이전 `src/engine/chat/`·`src/engine/core/` 구조가 아닌, 현재 `branch2`의 실제 모듈 위치만 설명합니다.

## 단일 실행 소유권

- `src/api/mod.rs`: 공통 Axum 진입점과 인증 미들웨어. 제품 API는 `src/api/surfaces/`로, 선택적 연결은 `src/api/addons.rs`로 분기합니다.
- `crates/dominion-turn-core/src/agent_loop.rs`: **필수 공통 Rust Harness**, 모델 요청·도구 응답 준비·배치 승인·독립 SDK 포트 실행.
- `crates/dominion-turn-core/src/driver.rs`: 라운드·현재 공개한 도구·중복 도구 ID·요청 예산에 대한 최종 승인 경계.
- `crates/dominion-turn-core/src/run_state.rs`: 작업의 공통 RunState/RunEvent 및 실행 phase 전이 계약.
- `crates/dominion-agent-loop/src/lib.rs`, `crates/dominion-agent-kernel/`: 교체 가능한 ReAct/Plan-Execute 전략과 결정·예산·스케줄링 알고리즘. 독립 모델/도구 효과 권한을 갖지 않습니다.
- `src/engine/services/conversation/chat/runtime/turn_body.rs`: 제품 Chat Host의 스트리밍·보존·정책 적응. 동일 Harness의 `prepare_response` / `admit_prepared`를 사용합니다.
- `src/engine/services/decision/runtime_program.rs`: 순수 ProgramIR/DAG 작업 편성·실행. 외부 효과는 같은 네이티브 도구 게이트웨이를 사용합니다.
- `src/engine/services/tools/tools.rs`: 실제 파일/터미널/MCP/외부 실행 도구 게이트웨이. `src/engine/services/operations/{effect,resource_lock}.rs`가 효과 영수증과 Writer 잠금/SQLite 임대를 소유합니다.
- `src/db.rs`와 `src/events.rs`: SQLite 작업 기록·이벤트 로그 및 SSE 재연결 커서의 사실 원천. 부모 실행 및 Task Team 자식 작업의 선점/완료는 SQLite 단일 트랜잭션에서 검증합니다.

## 그룹 대화와 작업 팀

- **Room Chat**: `src/engine/services/conversation/{rooms,room_state,room_workboard,room_parallel}.rs`. 발언권·영속 원문·참가자별 읽기 커서·개별 모델 요청. 병렬 모드는 읽기 전용으로 제한됩니다.
- **Task Team**: `crates/dominion-agent-loop/src/task_team.rs`의 DAG 파동 스케줄링, `src/engine/services/decision/task_team_work.rs`의 Worker 소유·변경 영수증·결과 병합. 쓰기/위임은 독점 파동입니다.

두 협업 방식은 하나의 Chat/Harness를 재사용하지만 별도의 조정 상태를 갖습니다. Room의 대화 문장을 작업 완료 증거로 취급하지 않습니다.

## 모델·외부 도구·UI

- `src/engine/services/providers/gateway.rs`: 로컬/API 모델의 기능 협상·대체 경로·토큰 기록.
- `src/engine/addons/`: MCP·외부 Agent Bridge·메시지 채널 등 선택적 연동. Codex/Claude/Qwen 실행기는 모델 제공자가 아닌 독립 Worker/Bridge입니다.
- `web/`, `src-tauri/`, `sdk/typescript/`: 동일 Axum API와 영속 이벤트 스트림을 소비하며 각각 에이전트 실행 루프를 복제하지 않습니다.
- 모델에 제공한 도구 호출, 승인 대기, 실행 성공·실패·결과 불명, 취소·재개와 최종 산출물은 Rust 호스트의 실행 계약으로 결정됩니다.

## 검증

- `node --test scripts/sub2-architecture-contract.test.mjs` — 1개 Harness·전략 분리·Task Team 트랜잭션·공통 이벤트·SDK 계약.
- `node --test scripts/sub2-core-boundary.test.mjs` — 기존 제품 Host·API 경계.
- `node --test scripts/sub2-addon-boundary.test.mjs` — 선택적 통합 경계.
- `node scripts/sub2-source-audit.mjs` — 소스/경로 중복.
- `cargo test --locked -p dominion-turn-core --lib`, `cargo test --locked -p dominion-agent-loop --lib`, `cargo test --lib engine::task_team_work::tests`, `cargo test --lib engine::resource_lock::tests`.
- `.github/workflows/ci.yml`은 개발용 검사이며 OS별 설치 패키지 빌드·실계정/실모델 장기 실행·상용 결제 검증은 별도입니다.

**사용자 메뉴, 메인 브랜치, HTTP URL을 변경하지 않고 Rust 내부 실행 책임과 일관성을 강화합니다.**
