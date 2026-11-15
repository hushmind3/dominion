# DOMINION 공통 Rust 실행 체계

메인의 SQLite·React 프론트·제품 API를 유지하면서, 서브4의 자체 Rust 상태머신과 도구 실행 알고리즘을 `crates/dominion-core`에 내장했습니다. 서브4의 별도 서버나 JSON 제품 저장소로 전환하지 않습니다.

## 실제 경로

`채팅 / 그룹 / 작업 / 워크플로 / RSI / SDK 자동 작업 → 메인 제품 어댑터 → dominion-core → 기존 모델·승인·도구 어댑터 → SQLite`

- `src/engine/chat/runtime.rs`는 제품 문맥·계약·RSI·학습 정책과 스트리밍을 연결합니다. 코어의 상태 전이·공유 예산·스키마 검증·실행 전 기록을 거쳐 도구를 실행합니다.
- `src/api/sdk.rs`의 기존 별도 모델 반복기와 파일 체크포인트 실행 경로는 제거했습니다. SDK 자동 작업도 일반 채팅과 같은 모델·인증·도구·검증을 사용합니다. Codex·Anthropic 등은 기존 메인 제공자 어댑터를 그대로 사용합니다.
- 네이티브 상태는 기존 SQLite의 `execution.kernelState`에 저장됩니다. 명시적인 병렬 프로그램 분기는 부모 예산을 공유하는 `native_frame`으로 기록하여 기존 동시 실행 기능을 보존합니다.
- 기존 `execution_operation`, 도구 승인, 파일 변경 잠금, 효과 재실행 방지, 목표 검증, 사용량·캐시 집계는 유지합니다.
- 결과가 불명확한 쓰기는 재시작 후 자동 재실행하지 않습니다. SDK 수동 관찰은 효과 기록·네이티브 상태·작업 상태를 한 SQLite 트랜잭션으로 갱신합니다.
- 모델의 단순 완료 주장으로 행동 목표를 성공 처리하지 않습니다. 기존 고정 파일 검증·필수 절차·독립 RSI 검증을 유지합니다.

## 제품 SDK API

| 메서드 | 경로 | 동작 |
|---|---|---|
| POST | `/api/sdk/runs` | `instruction`, `providerId?`, `model?`, `verifyCommand?`, `maxRounds?`로 시작 |
| GET | `/api/sdk/runs` | SQLite 작업 목록 |
| GET | `/api/sdk/runs/:id` | 공통 실행·승인·사용량·최종 응답 조회 |
| POST | `/api/sdk/runs/:id/approve` | `callId`에 해당하는 현재 승인만 처리 |
| POST | `/api/sdk/runs/:id/cancel` | 공통 세션 취소 |
| POST | `/api/sdk/runs/:id/reconcile` | 불명확한 효과 하나에 대한 사용자의 실제 관찰 기록 |
| POST | `/api/sdk/runs/:id/resume` | 불명확한 효과가 해소된 중단 작업을 명시적으로 재개 |

`verifyCommand`는 실행 전에 목표 검증 조건에 고정됩니다. 명령 실행에도 기존 승인 정책이 적용됩니다. `maxRounds`는 SDK 재개 이전의 모델 호출까지 누적하여 제한합니다.

사용자의 과거 실행 기록은 보존하며 자동 재실행하지 않습니다. 이전 독립 실행기·CLI 소스와 Cargo 워크스페이스 등록은 삭제했습니다. 제품과 공통 코어가 현재 워크스페이스의 전부입니다.

## 확인 범위

실제 로컬 파일·SQLite와 모의 제공자 HTTP를 사용하여 실행·승인·복구·캐시 집계를 검사합니다. 이 결과를 유료 모델의 실전 성능 향상이나 RSI 성능 향상으로 환산하지 않습니다.
