//! Bounded JSON Schema validation at the Rust tool-dispatch boundary.
//! Supports core MCP argument contracts without executing code or fetching
//! remote references. Unimplemented security-relevant schema constraints are
//! rejected instead of silently treated as successful validation.
use serde_json::Value;

pub fn validate_arguments(schema:&Value,arguments:&Value)->Result<(),String>{
 if arguments.to_string().len()>262_144 {
  return Err("Tool arguments exceed 256 KiB".into())
 }
 validate(schema,arguments,"$",0)
}
fn fail(path:&str,reason:&str)->Result<(),String>{
 Err(format!("Tool arguments at {path}: {reason}"))
}
fn matches_type(kind:&str,v:&Value)->bool {
 match kind{
  "object"=>v.is_object(),"array"=>v.is_array(),"string"=>v.is_string(),
  "boolean"=>v.is_boolean(),"null"=>v.is_null(),
  "integer"=>v.as_i64().is_some()||v.as_u64().is_some(),
  "number"=>v.is_number(),_=>false
 }
}
fn validate(schema:&Value,v:&Value,path:&str,depth:usize)->Result<(),String>{
 if depth>12{return fail(path,"schema nesting exceeds validation budget")}
 if schema==&Value::Bool(true){return Ok(())}
 if schema==&Value::Bool(false){return fail(path,"schema rejects every value")}
 let obj=schema.as_object().ok_or_else(||format!("Unsupported schema at {path}"))?;
 for key in ["$ref","$dynamicRef","pattern","patternProperties","not","if","then",
  "else","dependentSchemas","dependentRequired","unevaluatedProperties",
  "contains","propertyNames","contentEncoding","contentMediaType"] {
  if obj.contains_key(key){return fail(path,&format!("unsupported schema constraint {key}"))}
 }
 if let Some(t)=obj.get("type"){
  let supported=match t{
   Value::String(s)=>matches_type(s,v),
   Value::Array(variants)=>variants.iter().any(|entry|
    entry.as_str().is_some_and(|s|matches_type(s,v))),
   _=>false
  };
  if !supported{return fail(path,"incorrect JSON type")}
 }
 if let Some(expected)=obj.get("const"){
  if v!=expected{return fail(path,"const constraint mismatch")}
 }
 if let Some(items)=obj.get("enum").and_then(Value::as_array){
  if !items.contains(v){return fail(path,"enum constraint mismatch")}
 }
 if let Some(branches)=obj.get("allOf").and_then(Value::as_array){
  for branch in branches{validate(branch,v,path,depth+1)?}
 }
 if let Some(branches)=obj.get("anyOf").and_then(Value::as_array){
  if !branches.iter().any(|branch|validate(branch,v,path,depth+1).is_ok()){
   return fail(path,"none of anyOf schemas accepted this value")
  }
 }
 if let Some(branches)=obj.get("oneOf").and_then(Value::as_array){
  if branches.iter().filter(|branch|validate(branch,v,path,depth+1).is_ok()).count()!=1{
   return fail(path,"oneOf requires exactly one matching schema")
  }
 }
 if let Some(map)=v.as_object(){
  if let Some(min)=obj.get("minProperties").and_then(Value::as_u64){
   if map.len() < min as usize{return fail(path,"too few properties")}
  }
  if let Some(max)=obj.get("maxProperties").and_then(Value::as_u64){
   if map.len()>max as usize{return fail(path,"too many properties")}
  }
  if let Some(required)=obj.get("required").and_then(Value::as_array){
   for name in required{
    let key=name.as_str().ok_or_else(||format!("Invalid required schema at {path}"))?;
    if !map.contains_key(key){return fail(path,&format!("missing required property {key}"))}
   }
  }
  let properties=obj.get("properties").and_then(Value::as_object);
  for (key,value) in map {
   let next=format!("{path}.{key}");
   if let Some(spec)=properties.and_then(|p|p.get(key)){
    validate(spec,value,&next,depth+1)?;
   }else if let Some(additional)=obj.get("additionalProperties"){
    validate(additional,value,&next,depth+1)?;
   }
  }
 }
 if let Some(array)=v.as_array(){
  if let Some(min)=obj.get("minItems").and_then(Value::as_u64){
   if array.len()<min as usize{return fail(path,"too few array items")}
  }
  if let Some(max)=obj.get("maxItems").and_then(Value::as_u64){
   if array.len()>max as usize{return fail(path,"too many array items")}
  }
  if obj.get("uniqueItems")==Some(&Value::Bool(true)){
   for i in 0..array.len(){
    if array[i+1..].contains(&array[i]){return fail(path,"duplicate array items")}
   }
  }
  if let Some(items)=obj.get("items"){
   for (idx,value) in array.iter().enumerate(){
    validate(items,value,&format!("{path}[{idx}]"),depth+1)?;
   }
  }
 }
 if let Some(s)=v.as_str(){
  if let Some(min)=obj.get("minLength").and_then(Value::as_u64){
   if s.chars().count()<min as usize{return fail(path,"string too short")}
  }
  if let Some(max)=obj.get("maxLength").and_then(Value::as_u64){
   if s.chars().count()>max as usize{return fail(path,"string too long")}
  }
 }
 if let Some(n)=v.as_f64(){
  for (key,valid) in [
   ("minimum",Box::new(|x:f64,b:f64|x>=b) as Box<dyn Fn(f64,f64)->bool>),
   ("maximum",Box::new(|x:f64,b:f64|x<=b)),
   ("exclusiveMinimum",Box::new(|x:f64,b:f64|x>b)),
   ("exclusiveMaximum",Box::new(|x:f64,b:f64|x<b)),
  ]{
   if let Some(bound)=obj.get(key).and_then(Value::as_f64){
    if !valid(n,bound){return fail(path,&format!("{key} violated"))}
   }
  }
 }
 Ok(())
}
#[cfg(test)]
mod tests{
 use super::*;
 use serde_json::json;
 #[test]
 fn required_type_enum_nested_array_and_extra_properties_are_enforced(){
  let schema=json!({"type":"object","required":["id","labels"],
   "additionalProperties":false,"properties":{
    "id":{"type":"string","minLength":2},"labels":{"type":"array","minItems":1,
     "items":{"type":"integer","minimum":1,"maximum":10}},
    "mode":{"enum":["safe","dry_run"]}}});
  assert!(validate_arguments(&schema,&json!({"id":"abc","labels":[1,3]})).is_ok());
  for bad in [
   json!({"id":"abc"}),json!({"id":"x","labels":[1]}),
   json!({"id":"abc","labels":[0]}),json!({"id":"abc","labels":[1],"extra":1}),
   json!({"id":"abc","labels":["1"]}),json!({"id":"abc","labels":[1],"mode":"unsafe"})
  ]{assert!(validate_arguments(&schema,&bad).is_err(),"{bad}")}
 }
 #[test]
 fn reject_remote_schema_refs_and_unknown_constraints_instead_of_ignoring(){
  for s in [json!({"$ref":"https://example.com/evil"}),
   json!({"type":"string","pattern":"^ok$"}),
   json!({"type":"object","not":{"required":["secret"]}})] {
   assert!(validate_arguments(&s,&json!({})).is_err())
  }
  assert!(validate_arguments(&json!({"type":"object","additionalProperties":false}),
   &json!({})).is_ok());
 }
 #[test]
 fn exact_choice_and_json_size_constraints(){
  assert!(validate_arguments(&json!({"oneOf":[{"const":1},{"const":2}]}),&json!(2)).is_ok());
  assert!(validate_arguments(&json!({"oneOf":[{},{}]}),&json!(2)).is_err());
  assert!(validate_arguments(&json!({}),&json!({"x":"a".repeat(270000)})).is_err());
 }
}
