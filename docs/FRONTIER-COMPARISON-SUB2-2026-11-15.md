# 서브2: 2026 프런티어 실행 구조 대조 및 Rust 구현 추적
**기준일:** 2026-11-15 · **평가 대상:** 제품 실행 능력, 외부 모델 독립성, 사용자 지시 충실도, 재계획·검증·복구, 토큰/원가, GUI 및 공개 SDK

## 실제 경쟁 기준
참조 프로젝트의 별 수는 GitHub 공개 저장소 API 조회 당시 참고 신호일 뿐 성능 점수가 아니다.

| 시스템 | 2026-11-15 공개 GitHub 별 (조회 시점) | 공개 실행 구조에서 가져온 비교 원칙 | Dominion 서브2 실제 대응 |
| --- | ---: | --- | --- |
| [Codex Rust](https://github.com/openai/codex/blob/main/codex-rs/core/src/tasks/regular.rs) | 135,311 | `RegularTask::run_turns`: 모델 턴이 끝난 뒤 대기 중 입력을 확인하고 재진입, cancellation token 유지 | `src/engine/autonomy.rs::execute`가 작업 중 새 사용자 지시 revision을 읽고 재계획, `engine/jobs.rs::steer`로 DB 저장 |
| [Claude Code](https://claude.com/resources/articles/steering-claude-code-skills-hooks-rules-subagents-and-more) | 155,695 | 지시·규칙·스킬·훅·서브에이전트의 로딩 시점/권한/컨텍스트 비용 분리 | 기존 `engine/policy.rs`·`engine/skills.rs`·`engine/mcp.rs` 유지. 별도 새 SDK를 실행 정책으로 위임하지 않음 |
| [DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/core/agent-loop/README.md) | 281,155 | 세션 이벤트·모델/도구 실행·상태/라이프사이클 분리, 기본 루프 교체 가능 | `src/lib.rs` 공개 Rust SDK와 `api/*` 어댑터, `engine/jobs.rs` 실행 소유; `events.rs` 순서 보장 및 페이징 |
| [Qwen Code](https://github.com/QwenLM/qwen-code/blob/main/docs/developers/architecture.md) | 30,386 | UI와 CLI/SDK가 하나의 코어를 활용하고 사용자 후속 입력을 턴에 병합 | `src/main.rs`·`bootstrap.rs` 모두 `sdk::Dominion`의 native `start` 사용; TS 및 UI는 보조 |
| [OpenClaw](https://github.com/openclaw/openclaw/blob/main/docs/concepts/agent-loop.md) | 394,467 | 세션 단위 직렬화, 스트리밍, 사용량·권한·도구 실행, 중단·복구 경계 | 기존 세션·도구 경로 유지, 미확정 외부 효과에 대한 재시도 차단 및 `event_gap` |
| [Hermes](https://github.com/NousResearch/hermes-agent/blob/main/website/docs/user-guide/features/delegation.md) | 263,434 | 독립 판단·병렬 서브에이전트와 문맥을 분리하고 최종 결과만 상위에 반환 | 현재 `engine/rooms.rs`/서브태스크 경로와 비교 대상으로 관리. 동등한 격리·복구 실증은 아직 없음 |

## 사용자 목표 → 실행 → 증거의 실제 경로

```text
Rust SDK: sdk::Dominion::submit(Prompt) ───────────────────┐
React GUI: ExecutionMonitor → web/src/api.ts ─────────────┤
External TS SDK: sdk/typescript/src/index.ts ─────────────┤
                                                          ▼
POST /api/jobs → api/jobs.rs → engine/jobs.rs::enqueue → SQLite job
                                    │
                         engine/jobs.rs::run_job
                                    │
                          run_prompt (RSI 분기 보존)
                                    │ 일반 자율 작업
                         engine/autonomy.rs::execute
                       ┌────────────┼─────────────────┐
                검사 조건 동결      │             새 사용자 지시
             verification.rs        │       POST /api/jobs/:id/steer
                       │            │           engine/jobs.rs::steer
                       │     run_quiet_outcome         │
                       │            │           SQLite job revision
                       │      Rust chat/model/tool    │
                       └────────────┼─────────────────┘
                            증거로 성공 판정
                  미완료 → 집중 재계획 → 재실행 (최대 6회)
                  외부 효과 불확실 → needs_review
                  조건 충족 → completed + 검증 기록
                                    │
                         SQLite job / event_log
                                    │
                            GUI 기본 실행 기록/취소; 세부 검증은 API/SDK
```

- 원본 작업 지시와 추가 지시는 서로 덮어쓰지 않는다. `steeringRevision`은 별도 CAS 기록.
- `autonomousState`는 이전 시도·성공 도구·오류·미충족 조건을 checkpoint로 보관. 즉시 사용자 개입·취소가 가능하다.
- 독립 조건이 없으면 `verified:false`를 반환해 도구 활동만으로 완전한 검증이라고 주장하지 않는다.
- 검증 실패/실행 오류를 모델 자기평가에 맡기지 않고 Rust `verification::check_acceptance`가 다음 시도를 선택한다.
- 사용자 지시는 다음 모델 턴부터 반영된다. 이미 실행 중인 도구의 취소와 구별된다.
- `rsiSourceDir`가 설정된 자기개선 작업은 기존 분리 worktree와 평가/채택 경로로 보낸다. 이 경로를 일반 프롬프트 리페어가 덮어쓰지 않는다.
- Rust `sdk::Dominion`은 `AppState`와 동일한 job worker/SQLite를 직접 호출한다. TS SDK는 Rust가 실행하는 공개 HTTP의 보조 클라이언트다.

## 핵심 소스
1. `src/lib.rs`, `src/sdk.rs`, `src/main.rs`, `src/bootstrap.rs`: Rust SDK 공개 진입점 및 단일 런타임 부팅.
2. `src/engine/autonomy.rs`, `src/engine/jobs.rs`, `src/engine/verification.rs`: 단계별 재검증·재계획·중단/복구 정책.
3. `src/api/jobs.rs`, `src/api/mod.rs`, `sdk/typescript/src/index.ts`: 동일 실행의 보조 API 어댑터.
4. `web/src/components/ExecutionMonitor.tsx`: 메인 브랜치 원본의 작업 실행·기록·중단 UI. 자율 실행 세부 설정·중간 지시·검증 영수증은 독립 Rust SDK/API로 제공하며 서브2 전용 화면 변경은 채택하지 않음.
5. `src/events.rs`, `src/api/system.rs`: 순서 보장·분할 이벤트 재생 및 이력 만료 통지.

## 검증의 수준
- **코드 추가:** Rust 자율 루프의 결정을 검증하는 단위 테스트(검증 조건, 외부 효과로 인한 중단, 시도 한도, 사용자 지시 revision), 이벤트 복구 3개 경로 및 workspace_patch 증거 시험.
- **로컬 실행 확인:** TypeScript SDK 모의 HTTP/SSE 테스트 9/9 통과. 이는 TypeScript 전송 파서의 한정된 검증이며 Rust 실모델/전체 E2E 성능의 검증으로 대체하지 않는다.
- **미실행:** 본 작업 환경에 Rust toolchain이 없어 Rust 테스트/클리피/제품 빌드를 실행하지 못했다. `web` 의존성 설치도 없어 전체 React 타입 검사는 미실행. 병렬 서브에이전트·SWE-bench Verified·실제 사용자 동선/영상 품질·원가 비교도 미측정이다.
- **최종 판정 보류:** 프런티어 경쟁은 제품 통합 후 동일 모델·작업·도구·예산·하드웨어의 실제 결과로 해야 한다. 별/문서/테스트 개수만으로 동급을 주장하지 않는다.

## 다음 검증의 기준
빌드·타입체크·단일/다중 파일 패치→실제 명령→실패 수정·예상 밖의 도구 부작용→중단/복구·10개 이상 지시 전환·컨텍스트 장기 사용·토큰 캐시 사용량을 한 실행 기록으로 묶어 재현해야 한다. 패치가 중단 후 다시 적용되거나 외부 부작용이 중복되면 실패로 기록한다.

전체 제품에서는 영상 자동 생성의 완성본 품질/원가, 그룹 대화 연속성, RSI의 평가/채택/후손 개선, MCP/서브에이전트의 복구성과 비개발자 UX까지 같은 기준으로 측정한다.
