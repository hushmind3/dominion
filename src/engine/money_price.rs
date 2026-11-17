//! Money-only auditable spread calculations; no model, price feed or new DB.
//! A quote match does not establish an actual sale or buyer commitment.
use serde_json::{json,Value};
use chrono::{DateTime,NaiveDate};

fn amount(row:&Value,key:&str)->Option<f64>{
    row[key].as_f64().filter(|v|v.is_finite()&&*v>=0.0&&*v<=1e14)
}
fn required_amount(row:&Value,key:&str)->Option<f64>{
    amount(row,key).filter(|v|*v>0.0)
}
fn norm(v:&Value)->String{
    v.as_str().unwrap_or("").split_whitespace().collect::<Vec<_>>()
        .join(" ").to_lowercase()
}
fn date(v:&Value)->Option<NaiveDate>{
    let s=v.as_str()?;
    NaiveDate::parse_from_str(s,"%Y-%m-%d").ok().or_else(||
        DateTime::parse_from_rfc3339(s).ok().map(|t|t.date_naive()))
}
fn quoted_amount(quote:&str,amount:f64)->bool{
    quote.split(|c:char|!c.is_ascii_digit()&&c!='.'&&c!=',')
        .filter_map(|token|{
            let cleaned=token.trim_matches(|c|c=='.'||c==',').replace(',',"");
            cleaned.parse::<f64>().ok()
        }).any(|value| (value-amount).abs()<=amount.max(1.0)*1e-8)
}
fn foreign_rate(row:&Value,mut lookup:impl FnMut(&str)->Option<Value>)->Option<f64>{
    if row["currency"]=="KRW"{return Some(1.0)}
    let currency=row["currency"].as_str()?;
    if !matches!(currency,"USD"|"JPY"|"CNY"|"EUR"|"GBP"|"THB"|"TWD"){
        return None;
    }
    let rate=required_amount(row,"fxWonPerUnit")?;
    let fx_url=row["fxSourceUrl"].as_str()?;
    let fx_quote=row["fxQuote"].as_str()?;
    let fx_date=date(&row["fxObservedAt"])?;
    let price_date=date(&row["observedAt"])?;
    if (fx_date-price_date).num_days().abs()>7{return None;}
    let source=lookup(fx_url);
    let checked=crate::engine::money_citation::annotate(
        &json!({"url":fx_url,"quote":fx_quote}),source.as_ref());
    (checked["quotationStatus"]=="archived_page_quote_match"
        &&quoted_amount(fx_quote,rate)).then_some(rate)
}
/// Compares a buyer quotation with a supply quotation. Every amount must
/// appear in its actual archived excerpt; missing costs remain unknown.
/// Results are only an indicative spread, NEVER booked or realized profit.
pub fn compare(raw:&Value,mut lookup:impl FnMut(&str)->Option<Value>)->Value{
    if !raw.is_object(){return Value::Null;}
    let buyer=&raw["buyer"];let supplier=&raw["supplier"];
    let check=|row:&Value,source:Option<&Value>|{
        crate::engine::money_citation::annotate(row,source)
    };
    let buyer_url=buyer["url"].as_str().unwrap_or("");
    let supplier_url=supplier["url"].as_str().unwrap_or("");
    let a_source=lookup(buyer_url);let b_source=lookup(supplier_url);
    let a=check(buyer,a_source.as_ref());let b=check(supplier,b_source.as_ref());
    let mut status="incomplete_observations";
    let mut net=Value::Null;let mut gross=Value::Null;
    let buyer_amount=required_amount(buyer,"amount");
    let supplier_amount=required_amount(supplier,"amount");
    let buyer_qty=required_amount(buyer,"quantity");
    let supplier_qty=required_amount(supplier,"quantity");
    let a_date=date(&buyer["observedAt"]);let b_date=date(&supplier["observedAt"]);
    let a_kind=buyer["priceType"].as_str().unwrap_or("");
    let b_kind=supplier["priceType"].as_str().unwrap_or("");
    let valid_kind=|kind:&str|matches!(kind,"asking"|"wanted"|"listing"|"executed"|"indicative");
    if buyer_url.is_empty()||supplier_url.is_empty()||buyer_url==supplier_url {
        status="independent_sources_required";
    }else if a["quotationStatus"]!="archived_page_quote_match"
        ||b["quotationStatus"]!="archived_page_quote_match" {
        status="source_quote_unverified";
    }else if buyer_amount.is_none()||supplier_amount.is_none()
        ||buyer_qty.is_none()||supplier_qty.is_none() {
        status="amount_or_quantity_missing";
    }else if !quoted_amount(buyer["quote"].as_str().unwrap_or(""),buyer_amount.unwrap())
        ||!quoted_amount(supplier["quote"].as_str().unwrap_or(""),supplier_amount.unwrap()){
        status="quoted_price_mismatch";
    }else if !valid_kind(a_kind)||!valid_kind(b_kind){
        status="price_kind_missing";
    }else if norm(&buyer["unit"]).is_empty()||norm(&buyer["spec"]).is_empty()
        ||norm(&buyer["unit"])!=norm(&supplier["unit"])
        ||norm(&buyer["spec"])!=norm(&supplier["spec"]){
        status="unit_or_spec_mismatch";
    }else if (buyer_qty.unwrap()-supplier_qty.unwrap()).abs()>1e-8{
        status="lot_quantity_mismatch";
    }else if a_date.is_none()||b_date.is_none() {
        status="observation_date_missing";
    }else if (a_date.unwrap()-b_date.unwrap()).num_days().abs()>60{
        status="prices_not_contemporaneous";
    }else{
        let a_fx=foreign_rate(buyer,|url|lookup(url));
        let b_fx=foreign_rate(supplier,|url|lookup(url));
        if let (Some(a_fx),Some(b_fx))=(a_fx,b_fx){
            let buyer_won=buyer_amount.unwrap()*a_fx/buyer_qty.unwrap();
            let supplier_won=supplier_amount.unwrap()*b_fx/supplier_qty.unwrap();
            let diff=buyer_won-supplier_won;
            gross=json!(diff.round() as i64);
            let costs=&raw["costsPerUnitWon"];
            let fields=["shippingWon","dutyWon","platformFeeWon","returnsWon","laborWon"];
            if let Some(total)=fields.iter().try_fold(0.0,|acc,key|amount(costs,key).map(|v|acc+v)){
                net=json!((diff-total).round() as i64);
                status="indicative_spread_not_realized_profit";
            }else{status="additional_costs_missing";}
        }else{status="fx_source_unverified";}
    }
    json!({"status":status,"buyer":a,"supplier":b,
        "buyerCurrency":buyer["currency"],"supplierCurrency":supplier["currency"],
        "buyerPriceType":buyer["priceType"],"supplierPriceType":supplier["priceType"],
        "buyerObservedAt":buyer["observedAt"],"supplierObservedAt":supplier["observedAt"],
        "spec":buyer["spec"],"unit":buyer["unit"],
        "grossSpreadWonPerUnit":gross,"netSpreadWonPerUnit":net,
        "costBasis":"shipping_duty_platform_returns_labor_per_unit",
        "tradeConfirmed":false})
}
#[cfg(test)]
mod tests{
    use super::*;
    fn source(url:&str,text:&str)->Value{
        json!({"url":url,"id":url,"contentStatus":"page_fetched",
            "articleExcerpt":text,"contentRevisionId":format!("{url}:rev")})
    }
    fn sample()->Value{json!({
        "buyer":{"url":"https://buyer.test/p","quote":"Wanted price KRW 120000 for 10 units",
            "amount":120000,"currency":"KRW","priceType":"wanted",
            "quantity":10,"unit":"device","spec":"A-100","observedAt":"2026-11-16"},
        "supplier":{"url":"https://supplier.test/p","quote":"Asking price KRW 60000 for 10 units",
            "amount":60000,"currency":"KRW","priceType":"asking",
            "quantity":10,"unit":"device","spec":"A-100","observedAt":"2026-11-17"},
        "costsPerUnitWon":{"shippingWon":500,"dutyWon":200,"platformFeeWon":300,
            "returnsWon":100,"laborWon":400}
    })}
    fn lookup(url:&str)->Option<Value>{
        match url{
            "https://buyer.test/p"=>Some(source(url,"Wanted price KRW 120000 for 10 units")),
            "https://supplier.test/p"=>Some(source(url,"Asking price KRW 60000 for 10 units")),
            _=>None
        }
    }
    #[test]fn computes_only_indicative_spread_when_quotes_and_costs_align(){
        let value=compare(&sample(),lookup);
        assert_eq!(value["status"],"indicative_spread_not_realized_profit");
        assert_eq!(value["grossSpreadWonPerUnit"],6000);
        assert_eq!(value["netSpreadWonPerUnit"],4500);
        assert_eq!(value["tradeConfirmed"],false);
    }
    #[test]fn missing_fees_or_spec_comparability_never_invents_net_profit(){
        let mut value=sample();
        value["costsPerUnitWon"]["laborWon"]=Value::Null;
        let missing=compare(&value,lookup);
        assert_eq!(missing["status"],"additional_costs_missing");
        assert!(missing["netSpreadWonPerUnit"].is_null());
        value=sample();value["supplier"]["spec"]=json!("B-900");
        assert_eq!(compare(&value,lookup)["status"],"unit_or_spec_mismatch");
    }
    #[test]fn foreign_exchange_requires_archived_rate_quote(){
        let mut value=sample();
        value["supplier"]["currency"]=json!("USD");
        assert_eq!(compare(&value,lookup)["status"],"fx_source_unverified");
        value["supplier"]["fxWonPerUnit"]=json!(1500.0);
        value["supplier"]["fxSourceUrl"]=json!("https://unknown.fx/rate");
        value["supplier"]["fxQuote"]=json!("USD KRW 1500");
        value["supplier"]["fxObservedAt"]=json!("2026-11-17");
        assert_eq!(compare(&value,lookup)["status"],"fx_source_unverified");
    }
    #[test]fn prices_claimed_by_a_model_without_archived_quotes_are_not_comparable(){
        assert_eq!(compare(&sample(),|_|None)["status"],"source_quote_unverified");
    }
}