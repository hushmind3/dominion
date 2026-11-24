import {useLocale} from '../i18n'
import {rows,scalar,type EvidenceRow} from '../utils/intelligenceView'
export function RoomConflictSummary({resolution}:{resolution:EvidenceRow}){
 const en=useLocale()==='en',tr=(ko:string,eng:string)=>en?eng:ko
 return <details className="my-2 rounded-lg border border-[#E7E0D5] p-3 text-xs"><summary>{tr('에이전트 의견 충돌·판단 기록','Agent disagreement & resolution')}</summary>
 <p>{tr('선택 정책','Policy')}: {scalar(resolution.strategy)} · {tr('선호된 주장','Preferred claim')}: {scalar(resolution.outcome)}</p>
 <p>{tr('선택된 에이전트','Selected agent')}: {scalar(resolution.winner?.name||resolution.winner?.agentId)}</p>
 {rows(resolution.claims).map((c,i)=><div key={i} className="mt-1 border-t border-[#EFE9DF] pt-1">{scalar(c.agentId)} · {tr('주장 방향','Polarity')}: {scalar(c.polarity)} · {tr('판단 점수','Evidence score')}: {scalar(c.evidenceScore)}</div>)}
 <p className="mt-2 text-[#7C7469]">{tr('의견 선택은 실제 작업의 완료 검증과 별개입니다.','A preferred claim is not task completion verification.')}</p></details>
}
