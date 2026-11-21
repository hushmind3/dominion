# DOMINION Rust 실행 구조 (2026-11)

DOMINION의 실행 코어는 **Rust**입니다. 이 문서는 현재 저장소의 활성 코드와 운영 계약만 설명합니다. 과거 실험·내장 영상 제작·독립 RSI 실행 기록은 실행 기능으로 취급하지 않습니다.

## 1. 실행 커널·알고리즘·제품 호스트

| 계층 | 활성 구현 | 책임 |
| --- | --- | --- |
| 신뢰 가능한 실행 런타임 | `crates/dominion-agent-runtime/` | 요청·예산·도구 권한·영수증·체크포인트·상태 계약 |
| 에이전트 알고리즘 | `crates/dominion-agent-algorithms/` | 계획/도구 선택·문맥·실패 안내·ReActLoop |
| 외부 사용 SDK | `crates/dominion-agent-sdk/`, `sdk/rust/`, `sdk/typescript/` | Rust 커널 계약과 HTTP/SSE 클라이언트 |
| 제품 실행기 | `src/host/execution/native_agent.rs`, `jobs.rs` | SQLite, 모델·도구 연동, 실행 복구, 승인, 실측 |
| 대화·그룹 대화 | `src/conversation/chat/`, `src/features/collaboration/rooms.rs` | 스트리밍, 대화실, 메시지 소유권, 답변 순서 |
| 작업 팀 | `src/features/collaboration/task_team.rs`, `worktree.rs` | DAG, Worker 인계, 재시작, 작업공간 격리, 결과 검증 |
| 외부 CLI Bridge | `src/integrations/agent_bridge.rs` | 명시적으로 연결한 Codex·Claude Code·Qwen Code 읽기 전용 작업 |
| 확장·모델·저장 | `src/integrations/`, `src/features/providers/`, `src/storage/` | MCP, OAuth, 모델 라우팅, 영속 기록 및 데이터 경계 |

Chat의 스트리밍 턴은 별도 상위 어댑터를 유지합니다. 커널·도구 영수증 계약을 공유한다는 사실을 Chat·Room·Task의 모든 상위 실행 루프가 1개로 통합됐다는 뜻으로 해석하지 않습니다.

## 2. 실제 제품 진입점

- `POST /api/chat`: 사용자가 직접 보내는 대화 및 스트리밍 답변
- `POST /api/jobs`, `POST /api/sdk/runs`: 네이티브 Rust SDK 실행
- `POST /api/task-teams`: Lead/Worker DAG 실행, 하위 Worker 소유권·의존성 검증
- 기존 **작업 현황(Activity)** 화면: 팀 생성, Native/Qwen/Codex/Claude Worker 지정, 의존성·모델·증거·취소·재시도·Worktree 승인. **새 메뉴를 만들지 않습니다.**
- `POST /api/rooms/:id/run`: 여러 에이전트가 참여하는 방의 대화
- `GET /api/runs/:id`: Chat·Room·Task·SDK·Bridge의 공통 실행 상태 조회
- `GET /api/events?after=N`: 영속 이벤트 순번 기반 재연결 및 2초 간격 다른 프로세스 SQLite 이벤트 조회
- `/api/harness/runs`: 기존 클라이언트를 위한 **제한된 호환 API**. 새 요청은 네이티브 커널로 연결하고, 오래된 단독 실행 기록은 읽기 전용으로 유지합니다.

## 3. Worker 작업공간·병합

Task Team의 작업은 로컬 모델과 외부 API 모델을 Worker마다 별도로 지정할 수 있습니다. 읽기 Worker는 병렬 실행되며, 공유 작업공간의 쓰기 작업은 소유권이 확인된 영속 잠금으로 충돌을 제어합니다.

선택한 작업에 `isolation: "worktree"`를 지정하면 Git이 별도 체크아웃을 만듭니다. 실행 도구와 완료 검증은 해당 작업공간을 사용합니다. **원본 브랜치에 자동 반영되지 않습니다.** 승인형 적용 경로:

1. `GET /api/task-teams/:id/worktrees/:task/preview`: 패치 목록·해시 확인
2. `POST /api/task-teams/:id/worktrees/:task/promote`: 명시적 확인·해시 일치·충돌 검사 후 스테이징

자동 커밋·푸시·브랜치 덮어쓰기는 수행하지 않습니다. 복구 시 불명확한 외부 부작용은 재실행하지 않고 수동 검토 대상으로 남깁니다.

## 4. 기억·교정과 폐기된 자율 개선 기능

- **유지:** 사용자 원문에 기반한 기억, 명시적 정정·실패 기록, 수동 학습, 미승인 Skill 초안, 검증된 기존 절차, 에이전트별 성장 기록
- **제거:** 독립 RSI 캠페인·세대 반복·자동 소스 편집·자동 채택, 내장 영상/음성 생성·YouTube 업로드, 학습 모델이 자동 생성하는 실행 전략·도구 정책·하네스 코드 개선 제안
- **보존:** 기존 SQLite 작업·개선 이력, 대화 기록, 사용자 파일. 폐기된 작업 종류는 새 실행과 재시작 실행이 차단됩니다.

메모리 학습 모델은 근거 메시지 ID와 사용자 원문 인용을 요구합니다. 모델이 반환한 오래된 `strategies`, `toolPolicies`, `harnessProposal` 키는 이제 수동 기억 추출에서 무시합니다. 사용자가 명시적으로 내린 교정 사항은 별도의 기록·검증 절차를 따릅니다.

## 5. 연결과 운영 검증

브라우저, MCP, Google/GitHub OAuth, Discord/Telegram, 로컬 Skills는 `src/integrations/` 경계 아래에 있습니다. Codex·Claude Code·Qwen Code의 독립 Bridge는 기본 비활성화 상태이며 `DOMINION_EXTERNAL_AGENTS=1`에서 사용자가 연결해야 합니다. Qwen Code는 공식 Headless JSON 이벤트 배열과 읽기 전용 Plan 모드로 실행하며, 요청한 세션 ID와 결과 ID가 일치하는지 확인합니다. 실제 3종 CLI 인증·장시간 과금 검증과 운영체제별 Tauri 릴리즈 빌드는 별도로 확인해야 합니다.

2026-11-21 기준 Chat 스트리밍은 SDK의 모델/도구 예산과 영수증·저널 계약을 공유하지만, **Chat의 상위 모델 반복 루프는 Native ReActLoop와 아직 1개 코드 경로로 통합되지 않았습니다.** 작동하던 스트리밍·그룹 대화를 위험하게 갈아엎는 대신 실행 경계부터 단일화하는 중입니다.

자세한 내용:
- [현재 문서 목차](docs/INDEX.md)
- [Rust SDK와 호스트 경계](docs/CORE-BOUNDARY-2026-11-20.md)
- [경량화 및 폐기 기능 이력](docs/SUB3-LEAN-COMPLEXITY-2026-11-16.md)
- [Worktree 및 공통 실행 상태](docs/SUB3-WORKTREE-UNIFIED-RUN-2026-11-20.md)
- [데스크톱 설치/리소스 구성](docs/DESKTOP.md)
