//! Generic owner-only configuration proxy. No extension-specific schema or keys.
use super::*;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Change{confirmed:bool,values:Value}
fn url(port:u16)->String{format!("http://127.0.0.1:{port}/dominion/v2/configuration")}
async fn result(response:reqwest::Response)->Result<Value,String>{
 if response.status()==StatusCode::NOT_FOUND{return Err("This extension does not provide a configuration interface".into())}
 if !response.status().is_success(){return Err(format!("Extension configuration HTTP {}",response.status()))}
 crate::integrations::network::read_bounded_json(response,65536,"extension configuration").await
}
pub async fn read(State(state):State<AppState>,Path(id):Path<String>)->Response{
 let output:Result<Value,String>=async{
  let(row,port)=target(&state,&id).await?;
  result(client()?.get(url(port)).bearer_auth(v2::token(&state,&row)?).send().await.map_err(|e|e.to_string())?).await
 }.await;
 match output{Ok(v)=>Json(v).into_response(),Err(e)=>upstream(&e)}
}
pub async fn write(State(state):State<AppState>,Path(id):Path<String>,Json(input):Json<Change>)->Response{
 if !input.confirmed||!input.values.is_object()||input.values.to_string().len()>65536{return invalid("Confirm the extension configuration")}
 let _lock=match state.integrations.plugin_mutations.try_lock(){Ok(g)=>g,Err(_)=>return conflict("Extension change in progress")};
 if state.maintenance_mode(){return conflict("Maintenance mode")}
 if state.store.get(tools::GRANT,&id).is_some_and(|g|g["enabled"]==true){return conflict("Disable registered extension tools before changing configuration")}
 let output:Result<Value,String>=async{
  let(row,port)=target(&state,&id).await?;
  result(client()?.post(url(port)).bearer_auth(v2::token(&state,&row)?).json(&json!({"values":input.values,"confirmed":true})).send().await.map_err(|e|e.to_string())?).await
 }.await;
 match output{Ok(v)=>Json(v).into_response(),Err(e)=>upstream(&e)}
}
