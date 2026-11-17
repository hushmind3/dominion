export type MoneyPriceComparison={
 status?:string
 buyerCurrency?:string;supplierCurrency?:string
 buyerPriceType?:string;supplierPriceType?:string
 buyerObservedAt?:string;supplierObservedAt?:string
 spec?:string;unit?:string
 grossSpreadWonPerUnit?:number|null
 netSpreadWonPerUnit?:number|null
 tradeConfirmed?:boolean
 buyer?:{quotationStatus?:string}
 supplier?:{quotationStatus?:string}
}
const labels:Record<string,string>={
 independent_sources_required:'가격 출처의 독립성 확인 필요',
 incomplete_observations:'비교할 가격 자료가 부족함',
 source_quote_unverified:'두 원문의 가격 인용을 확인하지 못함',
 amount_or_quantity_missing:'가격 또는 수량이 빠짐',
 quoted_price_mismatch:'인용된 금액과 입력된 가격이 다름',
 price_kind_missing:'호가·희망가·실거래가 구분 필요',
 unit_or_spec_mismatch:'사양·단위가 달라 직접 비교 불가',
 lot_quantity_mismatch:'주문 수량이 달라 가격 기준 불일치',
 observation_date_missing:'가격 기준일 미확인',
 prices_not_contemporaneous:'가격 관측 시점 차이로 비교 보류',
 fx_source_unverified:'근거 있는 동시점 환율 확인 필요',
 additional_costs_missing:'운송·관세·수수료·반품·노동 비용 확인 필요',
 indicative_spread_not_realized_profit:'원문 가격에 근거한 가정상 차액 · 실거래 미확인'
}
const won=(v:number)=>v.toLocaleString('ko-KR',{maximumFractionDigits:0})+'원'
export function PriceSpread({value}:{value:MoneyPriceComparison}){
 const hasNet=value.status==='indicative_spread_not_realized_profit'
 return <div className="mt-3 rounded-lg border border-[#EADCC9] bg-[#FFF9EE] px-3 py-3 text-[11px] leading-5 text-[#6B5645]">
  <strong className="block text-xs text-[#805335]">원문 가격·총비용 비교 (모델 추측과 구분)</strong>
  <p className="mt-1">{labels[value.status||'']||'비교 상태 확인 필요'}</p>
  <p className="mt-1">구매측 {value.buyerPriceType||'미분류'} · {value.buyerCurrency||'통화 미확인'} · {value.buyerObservedAt||'시점 미확인'}</p>
  <p>공급측 {value.supplierPriceType||'미분류'} · {value.supplierCurrency||'통화 미확인'} · {value.supplierObservedAt||'시점 미확인'}</p>
  <p>비교 규격: {value.spec||'미확인'} / {value.unit||'미확인'}</p>
  {hasNet&&typeof value.grossSpreadWonPerUnit==='number'&&
   <p>부대비용 전 단위 차액: {won(value.grossSpreadWonPerUnit)}</p>}
  {hasNet&&typeof value.netSpreadWonPerUnit==='number'&&
   <strong className="mt-1 block text-xs text-[#805335]">
    배송·관세·수수료·반품·노동 비용 반영 후 단위 차액: {won(value.netSpreadWonPerUnit)}
   </strong>}
  <p className="mt-1 text-[#987C62]">원문 인용이 일치하더라도 실제 구매·매출·수익이 발생했다는 뜻은 아닙니다.</p>
 </div>
}
