# DOMINION Champion — 격리 RSI 실험 엔진 (2026-11-14)

## 실제 실행 경로

개발 스튜디오 격리 RSI 실험실 → POST /api/rsi/experiments → SQLite 영속 작업 큐 → server/rsi-engine.mjs → 모델 Provider가 편집 후보 생성 → 2개의 독립 Git worktree에서 동결 테스트 → 검증 기록 → 사용자 승인 → fast-forward 채택 → 후손 실험.

- UI: src/components/RsiPanel.tsx, src/pages/Developer.tsx
- HTTP/API: server/routes/rsi.mjs, server/index.mjs (관리자 토큰 검사 이후 실행)
- 작업 큐와 중단 복구: server/jobs.mjs
- 후보·격리·검증·채택: server/rsi-engine.mjs
- 영속 실험: SQLite records 테이블의 rsi_experiment 종류
- 작업 트리: runtime/rsi-worktrees/(실험 ID)/baseline, candidate
- 경험 활용: server/experience.mjs의 과거 검증·실패 전략을 후보 생성 문맥에 제공

## 사용 방법

1. 소스 개발 메뉴에서 별도 Git 소스 폴더를 연결하고 의존성을 설치합니다. 시스템 Git CLI가 PATH에 필요합니다.
2. 로컬 모델 또는 API Provider를 등록합니다.
3. 실험 목표와 수정 허용 소스 파일 경로 1~8개를 입력합니다. 예: server/model-router.mjs.
4. 테스트 방식은 node-tests(기본) 또는 npm-test를 선택합니다. 기준본과 후보본에 같은 명령과 동결된 테스트를 사용합니다.
5. 실험 결과의 기준/후보 테스트 통과 여부와 후보 커밋을 확인합니다. 통과한 후보도 사용자 승인 전에는 원본에 합쳐지지 않습니다.
6. 채택 이후 후손 실험 기능으로 부모 커밋과 전략을 이어받아 개선을 반복합니다.

## API

- POST /api/rsi/experiments — 요청: goal(문자열), paths(수정 허용 소스 파일 배열), test(node-tests 또는 npm-test), 선택적 providerId/model
- GET /api/rsi/experiments — 최근 실험 기록
- GET /api/rsi/experiments/:id — 상세 상태·후보 커밋·토큰·테스트 근거
- POST /api/rsi/experiments/:id/cancel — 취소
- POST /api/rsi/experiments/:id/accept — 검증 후보 채택
- POST /api/rsi/experiments/:id/next — 승인된 부모의 전략/경로를 계승하여 후손 생성

## 실험 안전·품질 불변 조건

- 모델은 원본 개발 체크아웃을 직접 수정하지 않습니다. 후보 Git worktree에서만 기존 파일의 정확히 한 번 등장하는 텍스트를 교체합니다.
- tests/, .github/, package.json, .env*, node_modules/ 등 평가 파일과 비밀/의존성은 후보 편집이 금지됩니다.
- 기준 커밋·평가 파일 해시를 사전에 동결합니다. 동결 평가 파일이 변하면 후보를 실패 처리합니다.
- 검증에는 기준 테스트 통과 여부와 후보 테스트 통과 여부가 모두 포함됩니다. 둘 다 통과했다면 추가 개선의 품질 이득은 승인자가 검토합니다.
- 채택하려면 원본이 깨끗해야 하고 HEAD가 최초 기준 커밋과 일치해야 합니다. 강제 푸시·자동 채택은 없습니다.
- 프로세스 종료로 중간에 멈춘 실험은 검토 대기로 바뀌고 모델 편집을 무조건 재실행하지 않습니다.
- 모델 가중치의 파인튜닝·LoRA·재학습은 RSI 경로에 연결하지 않습니다.
- 사용량은 모델 응답의 token usage를 기록합니다. 실제 원화 청구, GPU 비용과 캐시 적중률은 미계측입니다.

## 현재 한계

- Git worktree의 코드 테스트는 **운영체제 수준의 보안 샌드박스가 아닙니다.** 프로세스 실행 권한은 앱 사용자 권한을 따릅니다.
- 외부 Git CLI가 필요합니다. 기존 isomorphic-git 기반 개발 스튜디오와 별개의 선택 기능입니다.
- 기존 소스 node_modules를 worktree에 링크해 사용하므로 별도 컨테이너 격리·원격 실행 노드는 아직 없습니다.
- 소스 생성은 기존 파일의 JSON 문자열 치환 방식입니다. 새 파일 생성과 AST·의미 단위의 다중 후보 탐색은 추가 개발 대상입니다.
- 자동 코드 생성·테스트·후보 커밋까지 연결되었으며 승인 후 원본 반영을 수행합니다. 독립 LLM 심사, 비용/성능 회귀 평가, A→B→A 교차 영역 개선은 아직 미연결입니다.
- 현재 연결 저장소에는 dominion-sub1~4 브랜치가 없어 이들의 코드를 병합하지 않았습니다.

## 2026년 공개 설계 참고

- [Coding Agents and Verifiable Patches (2026-08-31)](https://gaos6e.github.io/AI-Agent-Engineer/en/environmental-agents/04-coding-agents-and-verifiable-patches): 버전 관리 환경과 diff·회귀 검증
- [Agent Harness Landscape](https://github.com/FlorianBruniaux/claude-code-ultimate-guide/blob/main/guide/ecosystem/agent-harness-landscape.md): 워크트리·체크포인트·재개 설계
- [OpenMontage](https://github.com/Nuos/code-OpenMontage): 제작 게이트·장면 단위 승인 등 향후 영상 파이프라인 참고

외부 소스 코드를 복사하지 않고 독립 구현했습니다.
