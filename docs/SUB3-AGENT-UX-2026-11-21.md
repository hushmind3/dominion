# DOMINION 서브3 — 실제 에이전트 실행 UX/UI 통합 (2026-11-21)

범위: `branch3` 브랜치만. `main` 병합·새 상단 메뉴·릴리스 빌드 없음.

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

## 전체 메뉴·반응형·접근성 고도화 (2026-11-21)

- 대상: 대화, 그룹 대화, 대시보드, 모델 관리, 에이전트, 연결·채널, 메모리, 실행 기록, 설정. 기존 9개 메뉴 유지.
- WorkspaceSearch.tsx: Ctrl/⌘ K 및 설정 헤더 검색으로 9개 메뉴를 찾는다. 위/아래, Enter, Esc 키보드 지원. 새 상단 메뉴 없음.
- WorkspaceShell.tsx / theme.css: 모바일 가로 메뉴에서 선택 항목을 보이는 위치로 자동 스크롤. 데스크톱에서 접힌 설정 사이드바가 모바일 메뉴를 감추지 않도록 보정. 포커스 및 화면 넘침·동작 감소 설정 개선.
- Dialog.tsx: 모달이 열리면 초점을 옮기고, 최상위 모달에서 Tab 순환, Escape 닫기, 닫은 후 기존 초점 복귀. 중첩 창은 최상위만 처리.
- Models.tsx / Connections.tsx: 긴 탭 목록 가로 스크롤 및 활성 탭 위치 유지.
- Home.tsx: 첫 시작 모델 연결 안내. Operations.tsx: 실행 가능한 작업 팀을 수동적 로그 앞에 배치.
- Chat.tsx: 데스크톱 → 휴대폰 화면 전환에서 열린 채팅 이력 목록이 대화를 가리는 문제 수정.

## 실제 브라우저 검증과 범위

- GitHub Actions의 browser-ux: Playwright Chromium과 Vite 개발 서버로 실제 React를 클릭·입력·스크린샷. 프로덕션 릴리스 빌드 없음.
- 검증 코드: web/scripts/workspace-visual.mjs 및 web/scripts/connection-visual.mjs.
- 1440/768/390/320px 4개 화면 너비에서 32회 메뉴 방문, 35개 스크린샷, 런타임 오류 0건.
- 단일 메뉴 검색 Ctrl+K → Enter → Esc, 설정 고급 펼치기, 키보드 포커스, 화면 가로 넘침, 자동 작업 알고리즘·실행 한도 요청, 팀 목표 폴백, 원격 장치 추론 요청을 테스트 픽스처로 검사.
- 연결 채널 플랫폼 선택·인증·대상 입력·검토·저장, MCP·스킬·플러그인 등의 전체 연결 탭은 별도 Chromium 시나리오에서 18장 추가 캡처. 총 53개 이미지.
- [기준 전체 CI 통과](https://github.com/hushmind3/dominion/actions/runs/44880099374): 프론트, Rust, Chromium 통과.
- 테스트 화면이 로딩 중인데 캡처하던 문제를 이후 수정해 실제 페이지 조작 요소가 보인 뒤 촬영한다. 데스크톱→모바일 채팅 서랍 문제도 실제 화면 증거를 바탕으로 수정했다. [후속 실제 브라우저 검증](https://github.com/hushmind3/dominion/actions/runs/44880383321).

실제 실행 브라우저는 GitHub Actions Linux Chromium이다. 사용자의 PC를 조작한 것은 아니며, 모델 비용 결제나 외부 서비스 인증은 수행하지 않는다. 외부 API는 결정적 테스트 픽스처로 대체했으므로 Safari/Firefox 및 실기기, 실제 원격 모델·유료 API·채널 송수신은 별도 운영환경 시험 대상이다.
