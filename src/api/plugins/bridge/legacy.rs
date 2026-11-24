//! Migration-only authentication for pre-existing v1 installations.
//! No bundled implementation or new installation path.
use crate::state::AppState;
use serde_json::Value;
pub(super) fn token(state:&AppState,row:&Value)->Option<String>{
    if !matches!(row["image"].as_str(),Some("dominion-voice:0.1.0"|"dominion-voice:0.2.0")){return None}
    let id=row["id"].as_str()?;
    if !super::valid_id(id){return None}
    state.read_secret(&format!("plugin:voice:{id}:bridge")).filter(|s|s.len()==64&&s.bytes().all(|b|b.is_ascii_hexdigit()))
}
