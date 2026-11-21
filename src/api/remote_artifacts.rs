//! Content-addressed cross-host patch handoff. Remote bytes are UNTRUSTED:
//! no git apply, workspace write, native success claim or command execution.
//! The coordinator journals provenance; owner review is a separate decision.
use crate::state::AppState;
use super::{remote_registry,remote_workers};
use axum::{
    extract::{Path,State},http::{HeaderMap,StatusCode},
    response::{IntoResponse,Response},Json,
};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};

const MAX_PATCH_BYTES:usize=128*1024;
fn reject(code:StatusCode,detail:&str)->Response{
    (code,Json(json!({"error":detail}))).into_response()
}
fn sha256(text:&str)->String{format!("{:x}",Sha256::digest(text.as_bytes()))}
fn valid_git_oid(raw:&str)->bool{
    (raw.len()==40||raw.len()==64)&&raw.bytes().all(|c|c.is_ascii_hexdigit())
}
fn artifact_id(job:&str)->String{format!("remote-artifact:{job}")}
fn public_manifest(value:&Value)->Value{
    json!({"id":value["id"],"jobId":value["jobId"],
        "workerId":value["workerId"],"baseCommit":value["baseCommit"],
        "patchSha256":value["patchSha256"],"patchBytes":value["patchBytes"],
        "status":value["status"],"nativeVerified":false})
}
/// Worker uploads exactly one patch for an active lease, with the same
/// machine-scoped credential as heartbeat. Failed hashes and stale leases are
/// rejected BEFORE any patch bytes can be admitted into the coordinator store.
pub async fn upload(State(state):State<AppState>,headers:HeaderMap,
    Path(job_id):Path<String>,Json(body):Json<Value>)->Response{
    let worker=body["workerId"].as_str().unwrap_or("");
    if let Err(error)=remote_registry::authenticate(&state,&headers,worker){return error;}
    let lease=body["leaseId"].as_str().unwrap_or("");
    let Some(patch)=body["patch"].as_str().filter(|s|
        !s.is_empty()&&s.len()<=MAX_PATCH_BYTES&&s.starts_with("diff --git "))else{
        return reject(StatusCode::BAD_REQUEST,"유효한 제한 크기의 Git 패치가 필요합니다.");
    };
    let Some(base)=body["baseCommit"].as_str().filter(|s|valid_git_oid(s))else{
        return reject(StatusCode::BAD_REQUEST,"유효한 Git 기준 커밋이 필요합니다.");
    };
    let digest=sha256(patch);
    if body["patchSha256"].as_str()!=Some(digest.as_str()){
        return reject(StatusCode::BAD_REQUEST,"패치 SHA-256이 일치하지 않습니다.");
    }
    if !state.store.get("remote_job",&job_id)
        .is_some_and(|row|remote_workers::lease_alive(&row,worker,lease)){
        return reject(StatusCode::CONFLICT,"만료되었거나 소유하지 않은 원격 임대입니다.");
    }
    let id=artifact_id(&job_id);
    let row=json!({"id":id,"kind":"remote_artifact",
        "schemaVersion":1,"status":"quarantined",
        "jobId":job_id,"workerId":worker,"leaseId":lease,
        "baseCommit":base,"patchSha256":digest,"patchBytes":patch.len(),
        "patch":patch,"nativeVerified":false,
        "createdAt":chrono::Utc::now().to_rfc3339()});
    // A second upload for this ticket cannot silently replace the first
    // candidate, even if a worker restarts or retries an HTTP response.
    let stored=match state.store.put_if_absent("remote_artifact",&row){
        Some(saved)=>saved,
        None=>match state.store.get("remote_artifact",&id){
            Some(previous) if previous["jobId"]==job_id
                &&previous["workerId"]==worker&&previous["leaseId"]==lease
                &&previous["baseCommit"]==base
                &&previous["patchSha256"]==digest&&previous["patch"]==patch=>previous,
            _=>return reject(StatusCode::CONFLICT,
                "이미 다른 내용의 원격 패치가 격리 보관되어 있습니다."),
        },
    };
    let associated=state.store.mutate_record("remote_job",&job_id,
        json!({"id":job_id}),|current|{
            if !remote_workers::lease_alive(current,worker,lease){
                return Err("stale lease".into());
            }
            if !current["artifactSha256"].is_null()
                &&current["artifactSha256"]!=digest{
                return Err("artifact already pinned".into());
            }
            current["artifactId"]=json!(id);
            current["artifactSha256"]=json!(digest);
            Ok(vec![])
        });
    let Ok(job)=associated else{
        return reject(StatusCode::CONFLICT,"패치 입고 중 임대가 만료되었습니다.");
    };
    // Publish ONLY manifest, not untrusted content or worker credentials.
    state.bus.record("remote_artifact",&public_manifest(&stored));
    state.bus.record("remote_job",&job);
    (StatusCode::CREATED,Json(public_manifest(&stored))).into_response()
}
/// Owner-only inspection returns patch bytes without touching the workspace.
/// The /api/remote/* read guard enforces the owner admin token.
pub async fn get(State(state):State<AppState>,Path(job_id):Path<String>)->Response{
    let Some(row)=state.store.get("remote_artifact",&artifact_id(&job_id))else{
        return reject(StatusCode::NOT_FOUND,"원격 패치를 찾을 수 없습니다.");
    };
    let Some(job)=state.store.get("remote_job",&job_id)else{
        return reject(StatusCode::NOT_FOUND,"원본 원격 작업이 없습니다.");
    };
    if job["artifactSha256"]!=row["patchSha256"]
        ||job["artifactId"]!=row["id"]||row["jobId"]!=job_id{
        return reject(StatusCode::CONFLICT,"원격 패치 출처가 일치하지 않습니다.");
    }
    Json(json!({"manifest":public_manifest(&row),"patch":row["patch"]})).into_response()
}
/// Reviewing the patch never applies it. A separate native verifier must
/// prove base commit, path safety, diff and acceptance checks before merge.
pub async fn accept(State(state):State<AppState>,Path(job_id):Path<String>,
    Json(body):Json<Value>)->Response{
    let approved=body["accept"].as_bool();
    if approved.is_none(){return reject(StatusCode::BAD_REQUEST,"accept 불리언 값이 필요합니다.");}
    let Some(job)=state.store.get("remote_job",&job_id)else{
        return reject(StatusCode::NOT_FOUND,"원격 작업을 찾을 수 없습니다.");
    };
    if job["status"]!="owner_accepted"{
        return reject(StatusCode::CONFLICT,"먼저 원격 실행 보고를 소유자가 승인해야 합니다.");
    }
    let id=artifact_id(&job_id);
    let changed=state.store.mutate_record("remote_artifact",&id,
        json!({"id":id}),|row|{
            if row["status"]!="quarantined"||row["jobId"]!=job_id
                ||job["artifactId"]!=row["id"]
                ||job["artifactSha256"]!=row["patchSha256"]
                ||sha256(row["patch"].as_str().unwrap_or(""))!=row["patchSha256"]{
                return Err("artifact provenance mismatch".into());
            }
            row["status"]=json!(if approved==Some(true){
                "owner_accepted"
            }else{"owner_rejected"});
            row["ownerReviewedAt"]=json!(chrono::Utc::now().to_rfc3339());
            row["nativeVerified"]=json!(false);
            Ok(vec![])
        });
    match changed{
        Ok(saved)=>{
            state.bus.record("remote_artifact",&public_manifest(&saved));
            Json(public_manifest(&saved)).into_response()
        }
        Err(_)=>reject(StatusCode::CONFLICT,"검토 가능한 패치 출처가 없습니다."),
    }
}

#[cfg(test)]
mod tests{
    use super::*;
    use crate::{config::Config,db::Store,vault::Vault};
    use axum::http::HeaderValue;
    fn fixture()->AppState{
        let root=std::env::temp_dir().join(format!("dominion-artifact-{}",
            uuid::Uuid::new_v4()));
        let cfg=Config{port:0,data_dir:root.clone(),web_dir:root.clone(),workspace:root};
        AppState::new(cfg.clone(),Store::open(&cfg.db_path()).unwrap(),
            Vault::open(&cfg.key_path()))
    }
    #[tokio::test]
    async fn exact_lease_patch_handshake_preserves_review_boundary(){
        let state=fixture();
        let reg=remote_registry::enroll(State(state.clone()),Json(json!({
            "workerId":"device-a","capabilities":["sdk","git"]}))).await;
        let data=axum::body::to_bytes(reg.into_body(),4096).await.unwrap();
        let token:Value=serde_json::from_slice(&data).unwrap();
        let mut headers=HeaderMap::new();
        headers.insert("x-dominion-worker-token",
            HeaderValue::from_str(token["token"].as_str().unwrap()).unwrap());
        state.store.put("remote_job",&json!({
            "id":"remote:one","status":"leased","kind":"remote_sdk",
            "workerId":"device-a","leaseId":"lease-a",
            "leaseExpiresAt":(chrono::Utc::now()+chrono::Duration::seconds(60)).to_rfc3339()
        })).unwrap();
        let patch="diff --git a/demo.txt b/demo.txt\n--- a/demo.txt\n+++ b/demo.txt\n@@ -1 +1 @@\n-old\n+new\n";
        let valid=json!({"workerId":"device-a","leaseId":"lease-a",
            "baseCommit":"a".repeat(40),"patchSha256":sha256(patch),"patch":patch});
        let mut corrupted=valid.clone();
        corrupted["patchSha256"]=json!("0".repeat(64));
        assert_eq!(upload(State(state.clone()),headers.clone(),
            Path("remote:one".into()),Json(corrupted)).await.status(),
            StatusCode::BAD_REQUEST);
        let mut spoofed=valid.clone();spoofed["leaseId"]=json!("wrong");
        assert_eq!(upload(State(state.clone()),headers.clone(),
            Path("remote:one".into()),Json(spoofed)).await.status(),
            StatusCode::CONFLICT);
        assert_eq!(upload(State(state.clone()),headers.clone(),
            Path("remote:one".into()),Json(valid.clone())).await.status(),
            StatusCode::CREATED);
        // Replayed identical uploads keep the same immutable artifact.
        assert_eq!(upload(State(state.clone()),headers.clone(),
            Path("remote:one".into()),Json(valid)).await.status(),
            StatusCode::CREATED);
        assert_eq!(state.store.all("remote_artifact").len(),1);
        assert_eq!(accept(State(state.clone()),Path("remote:one".into()),
            Json(json!({"accept":true}))).await.status(),StatusCode::CONFLICT);
        state.store.patch_if("remote_job","remote:one","status",
            &json!("leased"),&json!({"status":"owner_accepted"})).unwrap();
        let accepted=accept(State(state.clone()),Path("remote:one".into()),
            Json(json!({"accept":true}))).await;
        assert_eq!(accepted.status(),StatusCode::OK);
        let inspected=get(State(state.clone()),Path("remote:one".into())).await;
        let bytes=axum::body::to_bytes(inspected.into_body(),200_000).await.unwrap();
        let displayed:Value=serde_json::from_slice(&bytes).unwrap();
        assert_eq!(displayed["patch"],patch);
        assert_eq!(displayed["manifest"]["nativeVerified"],false);
        let _=std::fs::remove_dir_all(&state.config.data_dir);
    }
}
