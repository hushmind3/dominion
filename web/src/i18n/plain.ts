import {getLocale} from './index'

/**
 * The engine and third-party services speak in machine values ("error",
 * "HTTP 502", "Failed to fetch"). The owner should never read those. Every
 * surface that shows a state or a failure goes through this file, so wording
 * stays consistent and translated without touching each page.
 */

const koStatus: Record<string, string> = {
 ok: '정상',
 ready: '준비됨',
 running: '진행 중',
 starting: '시작 중',
 queued: '대기 중',
 planning: '계획 중',
 preparing: '준비 중',
 reviewing: '검토 중',
 repairing: '고치는 중',
 applying: '적용 중',
 checking: '확인 중',
 idle: '대기 중',
 paused: '일시정지',
 stopped: '중지됨',
 stopping: '멈추는 중',
 completed: '완료',
 succeeded: '완료',
 success: '완료',
 failed: '실패',
 failure: '실패',
 error: '확인 필요',
 issue: '확인 필요',
 setup: '설정 필요',
 unknown: '확인 필요',
 cancelled: '취소됨',
 interrupted: '중단됨',
 needs_review: '검토 필요',
 awaiting_approval: '승인 대기',
 needs_input: '입력 필요',
 disabled: '꺼짐',
 enabled: '켜짐',
 active: '활성',
 archived: '보관됨',
 connected: '연결됨',
 outbound: '보내기 전용',
 disconnected: '연결 안 됨',
 available: '사용 가능',
 unavailable: '사용 불가',
 installed: '설치됨',
 extracting: '압축 푸는 중',
 downloading: '내려받는 중',
 installing: '설치 중',
 verifying: '확인 중',
 budget_exceeded: '한도 초과',
 answered_unverified: '확인 전 답변',
 answered_verified: '확인된 답변',
}

const enStatus: Record<string, string> = {
 ok: 'OK', ready: 'Ready', running: 'Running', starting: 'Starting',
 queued: 'Queued', planning: 'Planning', preparing: 'Preparing',
 reviewing: 'Reviewing', repairing: 'Repairing', applying: 'Applying',
 checking: 'Checking', idle: 'Idle', paused: 'Paused', stopped: 'Stopped',
 stopping: 'Stopping', completed: 'Done', succeeded: 'Done', success: 'Done',
 failed: 'Failed', failure: 'Failed', error: 'Needs attention',
 issue: 'Needs attention', setup: 'Needs setup', unknown: 'Needs attention',
 cancelled: 'Cancelled', interrupted: 'Interrupted', needs_review: 'Review needed',
 awaiting_approval: 'Waiting for approval', needs_input: 'Input needed',
 disabled: 'Off', enabled: 'On', active: 'Active', archived: 'Archived',
 connected: 'Connected', outbound: 'Send only', disconnected: 'Not connected',
 available: 'Available', unavailable: 'Unavailable', installed: 'Installed',
 extracting: 'Extracting', downloading: 'Downloading', installing: 'Installing',
 verifying: 'Verifying', budget_exceeded: 'Budget exceeded',
 answered_unverified: 'Unverified', answered_verified: 'Verified',
}

export function plainStatus(status?: string | null): string {
 const raw = String(status ?? '').trim()
 if (!raw) return ''
 const table = getLocale() === 'en' ? enStatus : koStatus
 return table[raw.toLowerCase()] ?? (getLocale() === 'en' ? 'Needs attention' : '확인 필요')
}

type Rule = [RegExp, string, string]

const rules: Rule[] = [
 [/failed to fetch|networkerror|network request failed|load failed/i,
  '인터넷 연결을 확인한 뒤 다시 시도해 주세요.',
  'Check your connection and try again.'],
 [/timed? ?out|etimedout/i,
  '응답이 너무 늦습니다. 잠시 후 다시 시도해 주세요.',
  'That took too long. Try again in a moment.'],
 [/econnrefused|connection refused|connection reset/i,
  '서버에 연결할 수 없습니다. 잠시 후 다시 시도해 주세요.',
  'Cannot reach the server. Try again in a moment.'],
 [/http ?401|\bunauthorized\b|invalid token|invalid api key/i,
  '인증 정보가 올바르지 않습니다. 키나 토큰을 다시 확인해 주세요.',
  'The credentials are not valid. Check the key or token.'],
 [/http ?403|\bforbidden\b/i,
  '이 작업을 할 권한이 없습니다.',
  'You do not have permission to do that.'],
 [/http ?404/i,
  '찾을 수 없는 항목입니다. 새로고침한 뒤 다시 시도해 주세요.',
  'That item no longer exists. Refresh and try again.'],
 [/http ?429|rate limit/i,
  '요청이 너무 많습니다. 잠시 후 다시 시도해 주세요.',
  'Too many requests. Try again in a moment.'],
 [/http ?5\d\d/i,
  '서버가 잠시 응답하지 않습니다. 잠시 후 다시 시도해 주세요.',
  'The server is not responding. Try again in a moment.'],
 [/\bnot found\b/i,
  '찾을 수 없는 항목입니다. 새로고침한 뒤 다시 시도해 주세요.',
  'That item no longer exists. Refresh and try again.'],
 [/\bquota|exceeded your current quota|insufficient/i,
  '사용 한도를 넘었습니다. 결제 정보나 한도를 확인해 주세요.',
  'A usage limit was reached. Check billing or limits.'],
]

const generic = {ko: '문제가 생겼습니다. 다시 시도해 주세요.', en: 'Something went wrong. Try again.'}

/**
 * Korean messages from the engine already read like sentences, so they pass
 * through. Anything else is a machine value and gets a human sentence.
 */
export function plainError(message?: string | null): string {
 const raw = String(message ?? '').trim()
 const en = getLocale() === 'en'
 if (!raw) return en ? generic.en : generic.ko
 // A machine failure wins even when the engine wrapped it in Korean words
 // ("모델 목록 조회 실패 (HTTP 401 Unauthorized)").
 for (const [pattern, ko, english] of rules) {
  if (pattern.test(raw)) return en ? english : ko
 }
 if (/[가-힣]/.test(raw)) return raw
 // English locale keeps authored English messages; Korean locale never shows
 // an untranslated machine string.
 return en ? raw : generic.ko
}
