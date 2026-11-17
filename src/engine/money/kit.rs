//! Zero-token commercial test materials. Does not contact customers, deploy a
//! website, place orders or invent a payment method. Artifacts are editable
//! drafts for owner approval; the agent's facts remain unverified.
use serde_json::{json,Value};
fn field(value:&Value,max:usize)->String{
    value.as_str().unwrap_or("").trim().chars().take(max).collect()
}
fn safe_html(input:&str)->String{
    let mut result=String::new();
    for c in input.chars(){
        match c{
            '&'=>result.push_str("&amp;"),
            '<'=>result.push_str("&lt;"),
            '>'=>result.push_str("&gt;"),
            '"'=>result.push_str("&quot;"),
            '\''=>result.push_str("&#39;"),
            _=>result.push(c)
        }
    }
    result
}
fn contact(url:Option<&str>)->Option<String>{
    url.map(str::trim).filter(|v|v.starts_with("https://")&&v.len()<1500)
        .map(str::to_string)
}
pub fn create(candidate:&Value,campaign:&Value,contact_url:Option<&str>)->Result<Value,String>{
    let title=field(&candidate["title"],150);
    let offer=field(&candidate["offer"],800);
    if title.is_empty()||offer.is_empty(){
        return Err("제안 이름과 판매할 결과물이 있어야 합니다.".into());
    }
    let buyer=field(&candidate["customer"],200);
    let problem=field(&candidate["problem"],700);
    let why=field(&candidate["whyPay"],700);
    let mechanism=field(&candidate["mechanism"],900);
    let first_test=field(&candidate["firstTest"],900);
    let failure=field(&candidate["killSignal"],900);
    let price=candidate["economics"]["priceWon"].as_f64()
        .filter(|n|n.is_finite()&&*n>0.0&&*n<1e14)
        .map(|n|format!("{}원 (검증 전 예상 가격)",n.round() as i64))
        .unwrap_or_else(||"가격 협의 · 원가 및 지불의사 확인 전".to_string());
    let contact=contact(contact_url);
    let cta=if let Some(url)=&contact {
        format!("<a class=\"cta\" href=\"{}\" target=\"_blank\" rel=\"noopener noreferrer\">공급 가능성 문의하기</a>",
            safe_html(url))
    }else{
        "<span class=\"cta disabled\">문의 채널 연결 전 · 먼저 고객 인터뷰</span>".to_string()
    };
    let html=format!(r#"<!doctype html>
<html lang="ko"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<meta name="robots" content="noindex,nofollow"><title>{title}</title>
<style>
:root{{font-family:system-ui,-apple-system,"Apple SD Gothic Neo",sans-serif;color:#282827;background:#fcf9f4}}
*{{box-sizing:border-box}}body{{margin:0}}main{{max-width:900px;margin:auto;padding:60px 22px}}
.pill{{font-size:12px;letter-spacing:.2em;color:#bb6547;font-weight:800}}
h1{{font-size:clamp(30px,6vw,58px);line-height:1.13;letter-spacing:-.04em}}
p{{font-size:16px;line-height:1.8;color:#554c44}}
aside,section{{background:#fff;border:1px solid #e8dfd4;border-radius:20px;padding:26px;margin:24px 0}}
.cta{{display:inline-block;padding:14px 25px;border-radius:12px;background:#b65336;color:white;text-decoration:none;font-weight:750}}
.disabled{{background:#999;color:white}}
.grid{{display:grid;grid-template-columns:repeat(auto-fit,minmax(230px,1fr));gap:16px}}
small{{font-size:12px;color:#8a7b6c}}
</style></head><body><main>
<div class="pill">EARLY CUSTOMER VALIDATION · MARKET HYPOTHESIS</div>
<h1>{title}</h1>
<p>{offer}</p>
<div class="grid"><aside><small>대상 고객</small><h2>{buyer}</h2></aside>
<aside><small>예상 가격·계약 조건</small><h2>{price}</h2></aside></div>
<section><h2>해결할 문제</h2><p>{problem}</p></section>
<section><h2>왜 필요한가</h2><p>{why}</p><p>{mechanism}</p></section>
<section><h2>초기 테스트</h2><p>{first_test}</p>
{cta}<p><small>이 페이지는 시장 수요 검증용 초안입니다. 가격·제품 가능성·성과 주장은 아직 검증되지 않았습니다.</small></p></section>
<footer><small>사업 가설 검증 문서 · 자동 게시/구매/결제 없음</small></footer>
</main></body></html>"#,
        title=safe_html(&title),offer=safe_html(&offer),buyer=safe_html(&buyer),
        price=safe_html(&price),problem=safe_html(&problem),why=safe_html(&why),
        mechanism=safe_html(&mechanism),first_test=safe_html(&first_test),cta=cta);
    let brief=format!("# {title}\n\n- 지불 주체: {buyer}\n- 해결 문제: {problem}\n- 제안: {offer}\n- 왜 지불할까: {why}\n- 수익 구조: {mechanism}\n- 가격: {price}\n- 검증: {first_test}\n- 폐기 기준: {failure}\n- 출처/데이터: 모델 가설, 실제 판매 근거 확인 필요\n");
    let interview=format!("검증 대상: {buyer}\n1. 지금 {problem} 문제를 어떻게 해결하고 있나요?\n2. 현재 지급하는 실제 가격과 기회비용은 얼마인가요?\n3. {offer} 형태의 결과물을 누가 승인하고 구매하나요?\n4. 지금 유료로 구매할 의향이 있나요? 어느 조건에서 거절하나요?\n5. 초기 검증 기준: {first_test}\n실패 시 중단: {failure}\n");
    Ok(json!({"id":format!("kit:{}",uuid::Uuid::new_v4()),
        "campaignId":campaign["id"],"candidateId":candidate["id"],
        "name":title,"html":html,"briefMarkdown":brief,"interviewScript":interview,
        "contactUrl":contact,"deploymentStatus":"not_published",
        "verification":"draft_not_a_sale_or_prototype",
        "createdAt":chrono::Utc::now().to_rfc3339()}))
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test] fn generated_html_does_not_execute_source_content(){
        let c=json!({"id":"one","title":"<script>alert(1)</script>",
            "offer":"기기 <img src=x onerror=alert(1)>","customer":"B2B",
            "whyPay":"Saves labor"});
        let kit=create(&c,&json!({"id":"camp"}),None).unwrap();
        let html=kit["html"].as_str().unwrap();
        assert!(!html.contains("<script>alert"));
        assert!(!html.contains("<img src=x"));
        assert!(html.contains("&lt;script&gt;"));
        assert!(html.contains("noindex,nofollow"));
        assert_eq!(kit["deploymentStatus"],"not_published");
    }
    #[test] fn sales_material_does_not_invent_a_contact_channel(){
        let c=json!({"id":"one","title":"Robot","offer":"Factory robotics"});
        let kit=create(&c,&json!({"id":"camp"}),Some("javascript:alert(1)")).unwrap();
        assert_eq!(kit["contactUrl"],Value::Null);
    }
}
