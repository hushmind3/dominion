# DOMINION 서브3 — 실제 에이전트 실행 UX/UI 통합 (2026-11-21)

범위: `dominion-sub3` 브랜치만. `main` 병합·새 상단 메뉴·릴리스 빌드 없음.

## 핵심 동선

### 홈 → 자동 작업 (`web/src/components/NativeAgentPanel.tsx`)

1. 목표를 자연어로 입력해 기존 `POST /api/sdk/runs`으로 네이티브 Rust 실행을 시작한다.
2. 고급 설정에서 `standard-v1` / `focused-v1` 알고리즘과 `maxRounds`를 선택한다. 이 값은 실제 Rust 실행 요청에 포함된다.
3. 최근 30개 작업을 선택·새로고침한다. 선택 작업은 개별 `GET /api/sdk/runs/:id` 실행 기록과 실제 커널 체크포인트를 표시한다.
4. 진행 중인 작업에 추가 지시를 `POST /api/sdk/runs/:id/steer`(append)로 전달한다.
5. 승인·취소·안전한 재개는 서버가 반환한 상태를 표시한다. 낙관적 가짜 완료 표시를 하지 않는다.
6. 실제 수락 검증 결과(`verificationChecks`)와 토큰 사용량을 표시한다. 결과가 불확실한 도구 실행은 기존 `in_doubt` 수동 확인 절차를 따른다.

### 활동 → 작업 팀 (`web/src/pages/operations/TaskTeams.tsx`)

- 숨겨져 있던 작업 팀을 기본으로 노출. 기존 활동 페이지 내부에만 표시하며 새 탭·메뉴를 만들지 않는다.
- 한 개 Worker인 경우 목표가 Worker 지시문 기본값이 된다. 여러 Worker를 만들 때는 각 역할 지시문을 분리한다.
- 조사 Worker는 독립 병렬 작업. 수정 Worker를 추가하면 기존 모든 작업에 의존하게 해 완료된 조사 증거를 바탕으로 수정한다. 상한은 Rust 계약과 동일한 12개.
- 실제 `/api/task-teams` 상태와 Worker별 진행률(완료 수/전체 수), 검증 등급, 관찰된 도구 영수증을 표시한다.
- 기존 Git Worktree 변경 미리보기 → SHA-256 대조 → 사용자가 확인한 뒤 원본 인덱스 스테이징을 그대로 유지한다. 자동 커밋/병합 없음.

### 활동 → 작업 팀 안의 원격 장치 (`web/src/pages/operations/RemoteWorkers.tsx`)

- `GET /api/remote/workers`: 등록된 장치 이름, 로컬 모델 이름, 30초 heartbeat 기준 온라인 상태를 확인한다.
- `GET/POST /api/remote/tasks`, `POST /api/remote/tasks/:id/cancel`: 텍스트 추론 요청, 실행 현황, 결과, 취소를 같은 패널에서 관리한다.
- 503(비활성 게이트웨이)은 연결 실패 알림 도배 대신 비활성 안내를 노출한다. 토큰, 원격 키, API 비밀번호는 UI에 요구하지 않는다.
- 팀 Worker 실행 엔진에 `remote`를 추가했다. 온라인 원격 장치가 있는 경우에만 제출을 허용하고 읽기 전용·별도 기기 모델임을 표시한다.
- 백엔드 `src/api/task_teams.rs`는 텍스트 원격 워커를 `DOMINION_REMOTE_WORKERS=1`으로 게이트하고, Codex/Claude/Qwen CLI는 별도의 `DOMINION_EXTERNAL_AGENTS=1`으로 게이트한다.
- 원격 결과 `external_unverified`는 호스트 검증된 파일 변경과 섞지 않는다. 원격 작업은 도미니언의 셸·파일 도구 권한을 받지 않는다.

## UX 원칙

- 도구 실행, 모델 사용량, 작업 단계는 **모델이 말한 내용이 아니라 서버의 실행 기록**에서만 표시한다.
- 검토 대상 파일, 승인 대상 도구 호출, 원격 장치와 CI 동작을 명확히 구분한다.
- 코딩/팀 작업 과정과 외부 추론 과정은 서로 다른 권한 범위를 가진다.
- 모바일에서는 기존 반응형 그리드(한 열), 데스크톱에서는 2열 기록/상세 구성으로 표시한다.
- 비활성 원격 설정에서는 작업 요청을 차단하되 기존 로컬·API 모델 작업은 그대로 이용할 수 있다.

## 검사 범위

기존 `.github/workflows/sub3-product-acceptance.yml`이 `web/**` 변경에 대해 TypeScript 타입 검사, 웹 테스트, SDK 타입·HTTP 계약 테스트, Rust 제품 검사를 수행한다. 이 브랜치의 일반 CI는 데스크톱 설치 파일 또는 프론트 릴리스 빌드를 수행하지 않는다.

실제 OS별 시각적 스크린샷·실제 원격 장치 핸드셰이크는 별도 수동 검증 대상이다.
