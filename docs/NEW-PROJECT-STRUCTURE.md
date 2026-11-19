# 새 프로젝트 구조 설계안

상태: 제안(v0.1) · 2026-11-18
전제(가정): Rust 기반 로컬 우선 에이전트 실행 애플리케이션. 단일 바이너리 + 선택적 데스크톱 셸, SQLite 영속화, 1인~소규모 팀. 도메인이 다르면 3장의 경계 규칙만 유지하고 크레이트 이름을 교체한다.

## 1. 설계 원칙

1. **의존성은 한 방향으로만 흐른다**: `core ← engine ← (tools, store, providers) ← app`. 역방향 의존과 순환은 CI에서 금지한다.
2. **크레이트 경계 = 변경 이유**: 모델 제공자 교체, 저장소 교체, UI 교체가 서로의 코드를 건드리지 않아야 한다.
3. **부수효과는 경계에 격리**: 네트워크·파일·프로세스·시계·난수는 어댑터 뒤에 두고, 코어는 순수 함수와 명시적 입력으로 유지한다.
4. **실행 전 기록(record-before-effect)**: 외부 효과는 실행 전에 의도와 인자를 저널에 남기고, 결과·해제를 같은 레코드에 붙인다. 크래시 복구의 유일한 근거다.
5. **얇은 조립 계층**: `src/`는 설정 로드, 의존성 주입, 라우터 등록만 한다. 비즈니스 분기는 크레이트에 둔다.
6. **파일 하나가 커지면 경계가 틀린 신호**: 단일 파일 1,500줄, 단일 함수 80줄을 소프트 한도로 두고 초과 시 모듈 분리를 리뷰 항목으로 올린다. (현 워크스페이스의 `src/db.rs`가 약 112KB까지 자란 것은 반례로 기록)

## 2. 디렉터리 구조

```
myproject/
├── Cargo.toml                 # [workspace] + 공통 workspace.dependencies/lints
├── rust-toolchain.toml        # 툴체인 고정 (재현 가능한 빌드)
├── .cargo/config.toml         # 타깃별 링커/플래그
├── crates/
│   ├── app-core/              # 도메인 타입, 상태 머신, 순수 규칙. I/O 의존 0
│   │   └── src/{lib,domain,state,policy,error}.rs
│   ├── app-protocol/          # 경계 DTO: API 요청/응답, 이벤트, 에러 코드 (serde만)
│   ├── app-engine/            # 실행 루프: 예산, 취소, 재시도, 저널 오케스트레이션
│   │   └── src/{lib,run,budget,journal,delegation}.rs
│   ├── app-store/             # 저장소 trait + SQLite 구현 + 마이그레이션
│   │   └── src/{lib,traits,sqlite/,migrations/}
│   ├── app-tools/             # 도구 스키마, 인자 검증, 실행기, 샌드박스 정책
│   ├── app-providers/         # 모델/외부 API 어댑터 (reqwest, 스트리밍)
│   └── app-testkit/           # fake store/provider, 픽스처, 골든 헬퍼 (dev 전용)
├── src/                       # 바이너리: 조립만
│   ├── main.rs
│   ├── bootstrap.rs           # 설정 → 저장소 → 엔진 → 라우터 조립
│   ├── config.rs
│   └── api/                   # axum 라우터/핸들러 (얇게)
├── web/                       # 프론트엔드 (빌드 산출물은 배포 시에만 포함)
├── src-tauri/                 # 데스크톱 셸 (선택; 없으면 제거)
├── migrations/                # 스키마 버전. 코드와 분리해 diff 가능하게
├── tests/                     # 크레이트 경계를 넘는 통합/계약 테스트
├── bench/                     # 마이크로 벤치 + 측정 기록
├── scripts/                   # 개발·릴리스 자동화 (멱등하게)
├── docs/                      # 설계/결정 기록 (ADR), 운영 문서
└── .github/workflows/         # fmt → clippy -D warnings → test → 의존성 규칙 검사
```

## 3. 크레이트 책임과 경계 규칙

| 크레이트 | 책임 | 의존 가능 | 금지 |
|---|---|---|---|
| `app-core` | 도메인 타입, 상태 전이, 정책 판정 | 표준 라이브러리, serde, thiserror | tokio, axum, rusqlite, reqwest |
| `app-protocol` | 외부 노출 DTO와 이벤트 스키마 | serde, core | 저장소·네트워크·엔진 |
| `app-engine` | 실행 오케스트레이션, 예산/취소/재시도 | core, protocol, store trait, tools trait | 구체 저장소·구체 HTTP 클라이언트 |
| `app-store` | 영속화, 트랜잭션, 마이그레이션 | core, rusqlite | engine, api |
| `app-tools` | 스키마 검증, 도구 실행, 경로 탈출 차단 | core, protocol | engine, api |
| `app-providers` | 제공자별 요청/응답 변환, 재시도 분류 | core, protocol, reqwest | store, api |
| `src/` (bin) | 설정, DI, 라우팅, 종료 처리 | 전체 | 도메인 규칙 구현 |

추가 규칙:
- engine은 `dyn Store`, `dyn ToolRunner`, `dyn Provider` 트레이트에만 의존한다. 테스트는 `app-testkit`의 fake로 대체한다.
- 자격 증명·토큰은 `protocol`/로그에 절대 넣지 않는다. 로그는 구조화 필드(`run_id`, `tool`, `outcome`)만 남긴다.
- `unsafe`는 원칙적으로 금지(`#![forbid(unsafe_code)]`). 예외가 필요하면 해당 크레이트에만 허용하고 ADR로 사유를 남긴다.

## 4. 데이터 계층

- SQLite(WAL) + 명시적 마이그레이션. 마이그레이션은 append-only, 되돌리기 대신 전진 수정.
- 핵심 테이블 축: `run`(상태 머신), `execution`(호출/저널), `execution_operation`(외부 효과 증거), `event`(내구성 있는 이벤트 로그).
- 쓰기 트랜잭션은 "의도 기록 → 효과 실행 → 결과 확정"의 3단계를 따른다. 중간 크래시는 재시작 시 미확정 레코드로 탐지해 복구 또는 실패 처리한다.
- 조회는 읽기 전용 커넥션 풀, 쓰기는 단일 라이터로 직렬화해 `SQLITE_BUSY`를 줄인다.

## 5. 실행 경계(도구·샌드박스)

- 도구는 `schema`(모델에 보여준 그대로) + `validate` + `execute` 3요소로 정의하고, 검증은 실행 직전에 같은 스키마로 수행한다.
- 파일 접근은 워크스페이스 루트 기준 정규화 후 prefix 검사. 심볼릭 링크·`..`·절대경로 탈출을 테스트로 고정한다.
- 파괴적/과금 작업은 승인 토큰을 요구하고, 승인은 실행 전에 만료·범위와 함께 기록한다.
- 취소는 `CancellationToken`으로 전파하고, 취소 후에도 저널은 "취소됨"으로 닫는다(열린 채 남기지 않는다).

## 6. 테스트 전략

1. **단위**: core의 상태 전이·정책 판정. I/O 없이 밀리초 단위.
2. **계약**: store/provider/tool 트레이트별 공통 스위트를 구현체마다 재사용.
3. **골든**: 프롬프트 조립·직렬화 결과를 스냅샷으로 고정(변경 시 diff 리뷰).
4. **통합**: `tests/`에서 임시 SQLite + fake provider로 실행 루프 전체를 검증.
5. **복구**: 효과 실행 중 크래시를 주입해 재시작 복구 경로를 검증.
6. CI 게이트: `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test`, 의존성 방향 검사(`cargo deny`/`cargo metadata` 기반 스크립트).

## 7. 관측성과 운영

- `tracing` 구조화 로그 + 요청/실행 단위 `run_id` 상관관계.
- 실행별 비용·토큰·지연을 `bench/` 또는 전용 테이블에 누적해 회귀를 수치로 판단한다.
- 종료 신호 시 진행 중 실행을 취소하고 저널을 닫은 뒤 종료(그레이스풀 셧다운).

## 8. 도입 순서

- **Phase 0 (1~2일)**: 워크스페이스 골격, CI 게이트, `app-core` 타입과 상태 머신 테스트, `app-store` 최소 스키마.
- **Phase 1**: `app-engine` 실행 루프 + fake provider로 통합 테스트 통과. 이 시점에 API는 아직 없음.
- **Phase 2**: `app-providers` 실제 어댑터 1종, `app-tools` 파일 도구 + 탈출 방지 테스트, `src/api` 최소 라우트.
- **Phase 3**: `web` UI, 데스크톱 셸(필요 시), 복구·관측성 강화.

각 단계는 "테스트가 통과하는 수직 슬라이스"로 끝낸다. 가로로 모든 크레이트를 동시에 채우지 않는다.

## 9. 안티패턴(리뷰에서 즉시 반려)

- 코어 크레이트에서 `tokio::spawn` 또는 `reqwest` 직접 호출.
- 핸들러에서 SQL 직접 실행(저장소 계층 우회).
- 도구 스키마와 검증 로직의 이중 정의(드리프트 발생).
- 실패를 문자열로만 반환해 상위에서 파싱(에러 타입 열거 유지).
- 마이그레이션 파일 수정(이미 적용된 것).
