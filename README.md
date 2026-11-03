# DOMINION

로컬 우선 범용 AI 워크스페이스. **React 19 · TypeScript · Tailwind CSS 4 · Node.js 24 · SQLite · MCP**.

모델 검색·다운로드, 채팅, 에이전트, 회의실, 자동화, 메신저 및 외부 API 연결을 단일 UI에서 관리합니다. 외부 도구를 설치했다는 이유만으로 그 기능을 자체 구현했다고 표현하지 않습니다.

## 바로 실행하기

요구 사항: Node.js **24.12+**, npm, Git.

```bash
git clone https://github.com/hushmind3/dominion.git
cd dominion
npm install
npm run dev
```

개발 화면은 `http://127.0.0.1:5173`입니다. 배포 모드는 `npm run build && npm start`이며 `http://127.0.0.1:4317`을 사용합니다.

## 🧩 컴포넌트 아키텍처

```text
src/design/
  tokens.css        Tailwind v4 @theme · 색상/타이포/표면/그림자
  Primitives.tsx    버튼 · 입력 · 선택 · 토글 · 카드 · 표 · 모달 · 드로어
                    공용 레이아웃 · 탭 · 배지 · 프로그레스 · 빈 상태
  AppShell.tsx      데스크톱 접기/펼치기 · 모바일 메뉴 · 빠른 이동
  useResource.ts    API 상태/오류/재조회 공통 로직

src/features/
  Home.tsx          실제 작업공간 개요
  Models.tsx        모델 검색 · 설치 · 다운로드 · 실행 · 외부 API
  Chat.tsx          SSE 채팅 · 모델 선택 · 도구 승인 · 대화 세션
  ChatParts.tsx     메시지·세션·입력 UI 컴포넌트
  Agents.tsx        에이전트 · 그룹 회의
  Workflows.tsx     순차 작업
  Memory.tsx        지속 기억
  Connections.tsx   메신저 · OAuth
  Extensions.tsx    MCP · 스킬
  Capabilities.tsx  실행 가능한 기능 목록
  Operations.tsx    자동화 · 실행 기록 · 권한

src/pages/          화면 구현 없이 feature를 재노출하는 라우트 경계
src/style.css       사용하지 않는 빈 레거시 파일
```

화면별 손 CSS 대신 **공통 디자인 컴포넌트와 Tailwind 유틸리티**를 사용합니다. UI 색상·간격의 기준은 `src/design/tokens.css`에 정의되어 있습니다. 모든 페이지는 공유 UI 조합으로 구성됩니다.

## 모델 라이브러리 · 실제 지원 범위

DOMINION 모델 라이브러리는 Hugging Face 공개 모델 저장소를 검색하여 파일/메타데이터를 가져오고, Node 백엔드가 모델 파일을 **자체 다운로드·저장·관리**합니다.

- GGUF 양자화별 파일 선택, MLX safetensors 스냅샷 묶음 선택
- 디스크 크기, README, 원본 리비전 확인
- 다운로드 진행률, 일시정지, 재개, 중단, 로컬 모델 삭제
- 설치 파일 목록 영속 저장(`runtime/models`)
- 로컬 모델 실행 상태, RAM·VRAM 추정 정보
- 챗 화면에서 실제로 실행 중인 모델/외부 API 모델 선택

**추론 계산 라이브러리는 별도입니다.** GGUF는 호환 `llama-server` 실행 파일(llama.cpp 기반), Apple Silicon MLX는 `python3 -m mlx_lm.server` (`mlx-lm`)을 호출합니다. 해당 바이너리·패키지가 없으면 설치한 모델을 보존하면서 실행 버튼을 비활성화합니다. DOMINION은 추론 커널 전체를 처음부터 구현한 엔진이라고 주장하지 않습니다.

- `DOMINION_LLAMA_SERVER`: GGUF 추론 서버 실행 파일 경로
- `DOMINION_MLX_PYTHON`: MLX용 Python 실행 파일 경로
- gated 모델, 인증이 필요한 파일, 모델 전용 커스텀 커널·미지원 양자화는 추가 호환성 작업이 필요합니다.
- 일부 모델은 표준 추론 서버 대신 해당 모델 제작사가 배포한 전용 커널을 필요로 합니다.
- GGUF 모델이 여러 개의 샤드로 배포되는 경우 별도의 런타임 지원 확인이 필요합니다.

클라우드/원격 모델은 모델 라이브러리의 **외부 API · 선택 사항**에서 추가할 수 있습니다. 기본 앱 설치를 전제로 하거나 외부 프로그램을 자동 등록하지 않습니다.

## 실행과 검증

```bash
npm run dev            # React 개발 화면 + Node API
npm run check          # TypeScript, Node 테스트, 배포 빌드
npm run test:ui         # 실제 Chrome 기반 브라우저 스모크 테스트
npm start               # 컴파일된 웹 화면과 API 제공
```

`tests/frontend-contract.test.mjs`는 **src/style.css = 0바이트**, 페이지의 직접 DOM 컨트롤/손 CSS 없음, 디자인 토큰 정의를 자동 검증합니다.

`tests/ui-browser-smoke.mjs`는 데스크톱 사이드바 접기·펼치기, 모델 목록과 버전 선택, 채팅 입력, 에이전트 생성, 모바일 탐색을 실제 헤드리스 Chrome에서 검사합니다. 브라우저 검사의 모델 메타데이터는 재현 가능한 테스트 응답을 사용합니다.

백엔드 자동검사는 네트워크와 디바이스를 모의하므로 **실제 Mac/Windows에서 GPU에 모델을 로드하고 텍스트를 생성한 검증을 대신하지는 않습니다.**

## 데이터와 보안

실행 파일은 `workspace/`, 로컬 데이터베이스 및 모델 파일은 `runtime/`에 저장됩니다. 환경 설정은 `.env`로 관리할 수 있습니다. 원격 바인딩은 `DOMINION_ADMIN_TOKEN`이 필요합니다. 도구 승인과 모델 제공자 인증 정보는 서버에서 관리합니다.

현재 백엔드는 Node.js의 FastAPI 없는 직접 HTTP/SSE 구현입니다. 본 저장소는 공개 상태이며 개인 토큰·대화 데이터·모델 파일을 커밋하지 않아야 합니다.

자세한 기존 기술 문서는 [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md), [docs/STATUS.md](docs/STATUS.md)를 참고하세요.
