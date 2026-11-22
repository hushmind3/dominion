/**
 * Registry cards come from a public catalog whose descriptions are written in
 * English for developers. The owner sees a Korean one-liner first; the original
 * text stays available behind "원문 보기".
 */

const curated: Array<[string, string]> = [
  ['github', 'GitHub 저장소의 파일·이슈·PR을 다루는 도구입니다.'],
  ['gitlab', 'GitLab 프로젝트의 파일과 이슈를 다루는 도구입니다.'],
  ['playwright', '웹 페이지를 열고 조작하거나 화면을 캡처하는 도구입니다.'],
  ['puppeteer', '웹 페이지를 자동으로 조작하는 도구입니다.'],
  ['browser', '웹 페이지를 열어 내용을 확인하는 도구입니다.'],
  ['filesystem', '내 컴퓨터의 파일을 읽고 정리하는 도구입니다.'],
  ['file', '파일을 읽고 저장하는 도구입니다.'],
  ['slack', 'Slack 메시지를 읽고 보내는 도구입니다.'],
  ['discord', 'Discord 메시지를 읽고 보내는 도구입니다.'],
  ['notion', 'Notion 문서를 읽고 쓰는 도구입니다.'],
  ['confluence', 'Confluence 문서를 검색하고 읽는 도구입니다.'],
  ['postgres', '데이터베이스에 질문하고 결과를 받는 도구입니다.'],
  ['mysql', '데이터베이스에 질문하고 결과를 받는 도구입니다.'],
  ['sqlite', '데이터베이스에 질문하고 결과를 받는 도구입니다.'],
  ['mongo', '데이터베이스의 자료를 조회하는 도구입니다.'],
  ['redis', '저장된 임시 데이터를 조회하는 도구입니다.'],
  ['search', '웹에서 정보를 검색하는 도구입니다.'],
  ['serp', '웹 검색 결과를 가져오는 도구입니다.'],
  ['gmail', '메일을 읽고 보내는 도구입니다.'],
  ['mail', '메일을 읽고 보내는 도구입니다.'],
  ['calendar', '일정을 확인하고 추가하는 도구입니다.'],
  ['drive', '드라이브의 문서를 찾고 읽는 도구입니다.'],
  ['figma', '디자인 파일을 읽는 도구입니다.'],
  ['youtube', '영상 정보와 자막을 가져오는 도구입니다.'],
  ['weather', '날씨를 확인하는 도구입니다.'],
  ['stripe', '결제 정보를 다루는 도구입니다.'],
  ['jira', 'Jira 이슈를 조회하고 갱신하는 도구입니다.'],
  ['linear', 'Linear 이슈를 조회하고 갱신하는 도구입니다.'],
  ['sentry', '오류 기록을 조회하는 도구입니다.'],
  ['vercel', '배포 상태를 확인하는 도구입니다.'],
  ['cloudflare', 'Cloudflare 설정과 기록을 확인하는 도구입니다.'],
  ['docker', '실행 중인 컨테이너를 확인하는 도구입니다.'],
  ['kubernetes', '서버 클러스터 상태를 확인하는 도구입니다.'],
  ['todoist', '할 일 목록을 읽고 추가하는 도구입니다.'],
  ['spotify', '음악 정보를 조회하는 도구입니다.'],
  ['maps', '지도와 장소 정보를 조회하는 도구입니다.'],
  ['pdf', 'PDF 문서를 읽고 요약하는 도구입니다.'],
  ['memory', '대화 내용을 기억해 두는 도구입니다.'],
  ['time', '현재 시간과 시간대를 알려주는 도구입니다.'],
  ['fetch', '웹 페이지 내용을 가져오는 도구입니다.'],
]

const kinds: Array<[RegExp, string]> = [
  [/database|postgres|mysql|sql|mongo|redis|query/i, '데이터베이스'],
  [/browser|scrape|crawl|screenshot|playwright|puppeteer/i, '웹 브라우저'],
  [/git(hub|lab)?|pull request|repository/i, '저장소'],
  [/slack|discord|teams|chat|message/i, '메신저'],
  [/docs?|wiki|notion|confluence/i, '문서'],
  [/search|serp|web/i, '검색'],
  [/file|storage|drive|s3/i, '파일'],
  [/mail|gmail|email|calendar/i, '메일·일정'],
  [/payment|stripe|billing/i, '결제'],
  [/model|llm|ai|inference|agent/i, 'AI'],
]

export function koreanToolSummary(name: string, description?: string): string {
  const key = `${name} ${description || ''}`.toLowerCase()
  for (const [needle, text] of curated) {
    if (name.toLowerCase().includes(needle)) return text
  }
  const kind = kinds.find(([pattern]) => pattern.test(key))
  return `「${name}」 · ${kind ? kind[1] : '외부 서비스'} 도구입니다.`
}
