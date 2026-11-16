//! Hugging Face discovery for video models that are not bundled into Dominion.
//! Searching / downloading an artifact does not claim that a ComfyUI workflow
//! for this checkpoint has been configured or proved on the owner's GPU.
use crate::state::AppState;
use futures_util::StreamExt;
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::{path::Path,time::Duration};

pub const FEATURED: [(&str,&str);6]=[
    ("Wan-AI/Wan2.2-TI2V-5B","Wan 2.2 · 영상 생성 5B"),
    ("Lightricks/LTX-Video","LTX-Video · 빠른 영상 생성"),
    ("Lightricks/LTX-2.3","LTX-2.3 · 영상·음성 생성"),
    ("tencent/HunyuanVideo","HunyuanVideo · 영상 생성"),
    ("THUDM/CogVideoX-5b","CogVideoX · 영상 생성"),
    ("genmo/mochi-1-preview","Mochi 1 · 영상 생성"),
];
const MAX_WEIGHT_BYTES:u64=150*1024*1024*1024;
fn repo_parts(repo:&str)->Result<(&str,&str),String>{
    if repo.len()>180{return Err("모델 저장소 ID가 너무 깁니다.".into())}
    let (owner,name)=repo.split_once('/').ok_or("Hugging Face 저장소는 owner/model 형식이어야 합니다.")?;
    let safe=|part:&str| !part.is_empty()&&part.len()<=100
        &&part!="."&&part!=".."&&!part.starts_with('.')
        &&part.chars().all(|c|c.is_ascii_alphanumeric()||matches!(c,'-'|'_'|'.'));
    if !safe(owner)||!safe(name)||name.contains(".."){
        return Err("모델 저장소 경로가 올바르지 않습니다.".into());
    }
    Ok((owner,name))
}
fn weight_path(path:&str)->bool{
    if path.len()>320||path.starts_with('/')||path.contains('\\')||path.contains('%')||
       path.split('/').any(|part|part.is_empty()||part=="."||part==".."||part.starts_with('.')){
        return false
    }
    if !path.chars().all(|c|c.is_ascii_alphanumeric()||matches!(c,'_'|'-'|'.'|'/')) {
        return false
    }
    let lower=path.to_ascii_lowercase();
    [".gguf",".safetensors",".ckpt",".pt"].iter().any(|ext|lower.ends_with(ext))
}
fn model_is_video(info:&Value,repo:&str)->bool{
    let pipeline=info["pipeline_tag"].as_str().unwrap_or("");
    let tags=info["tags"].as_array().map(|tags|tags.iter()
        .filter_map(Value::as_str).collect::<Vec<_>>()).unwrap_or_default();
    if matches!(pipeline,"text-to-video"|"image-to-video"|"video-to-video") ||
        tags.iter().any(|tag|matches!(*tag,"text-to-video"|"image-to-video"|"video-to-video")){
        return true
    }
    // Some official and community repos store GGUF weights without task tags.
    // Only recognizable video-generation family names qualify for untagged
    // browsing, never generic LLM repositories or arbitrary code bundles.
    let lower=repo.to_ascii_lowercase();
    ["wan2.","wan_", "ltx-video","ltx-2", "hunyuanvideo","cogvideox","mochi-1",
     "skyreels","hunyuan-video"].iter().any(|term|lower.contains(term))
}
fn hub_url(repo:&str,subpath:&[&str])->Result<reqwest::Url,String>{
    let (owner,name)=repo_parts(repo)?;
    let mut url=reqwest::Url::parse("https://huggingface.co/")
        .map_err(|e|e.to_string())?;
    url.path_segments_mut().map_err(|_|"Hub 주소 오류")?
        .extend([owner,name]).extend(subpath.iter().copied());
    Ok(url)
}
fn client()->Result<reqwest::Client,String>{
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(30))
        .build().map_err(|e|e.to_string())
}
async fn fetch_model(client:&reqwest::Client,repo:&str)->Result<Value,String>{
    let url=hub_url(repo,&[])?;
    let mut url=url;
    url.set_path(&format!("/api/models/{}",url.path().trim_start_matches('/')));
    let response=client.get(url).send().await.map_err(|e|e.to_string())?;
    if !response.status().is_success(){return Err(format!("모델 메타데이터 조회 실패: HTTP {}",response.status()))}
    response.json::<Value>().await.map_err(|e|e.to_string())
}
fn model_row(info:&Value)->Option<Value>{
    let repo=info["id"].as_str()?;
    if repo_parts(repo).is_err()||!model_is_video(info,repo){return None}
    let task=info["pipeline_tag"].as_str().unwrap_or("video");
    Some(json!({"id":repo,
        "name":repo.rsplit('/').next().unwrap_or(repo),
        "family":family(repo),"task":task,
        "downloads":info["downloads"].as_u64().unwrap_or(0),
        "likes":info["likes"].as_u64().unwrap_or(0),
        "gated":info["gated"]!=false&& !info["gated"].is_null(),
        "url":format!("https://huggingface.co/{repo}"),
        "source":"huggingface","runtime":"workflow_required"}))
}
fn family(repo:&str)->&'static str{
    let name=repo.to_ascii_lowercase();
    if name.contains("wan"){"Wan"}else if name.contains("ltx"){"LTX"}
    else if name.contains("hunyuan"){"Hunyuan"}else if name.contains("cogvideo"){"CogVideoX"}
    else if name.contains("mochi"){"Mochi"}else if name.contains("skyreels"){"SkyReels"}
    else{"기타 영상 모델"}
}
pub async fn search(query:&str)->Result<Value,String>{
    let q=query.trim();
    if q.chars().count()>90{return Err("검색어는 90자까지 입력할 수 있습니다.".into())}
    if q.is_empty(){
        return Ok(json!({"models":FEATURED.iter().map(|(repo,name)|
            json!({"id":repo,"name":name,"family":family(repo),"task":"video",
                "gated":false,"url":format!("https://huggingface.co/{repo}"),
                "source":"featured","runtime":"workflow_required"})).collect::<Vec<_>>(),
            "source":"featured","query":q}));
    }
    let client=client()?;
    let mut url=reqwest::Url::parse("https://huggingface.co/api/models")
        .map_err(|e|e.to_string())?;
    url.query_pairs_mut().append_pair("search",q)
        .append_pair("sort","downloads").append_pair("direction","-1")
        .append_pair("limit","80").append_pair("full","true");
    let response=client.get(url).send().await.map_err(|e|format!("모델 검색 서버에 연결할 수 없습니다: {e}"))?;
    if !response.status().is_success(){return Err(format!("영상 모델 검색 실패: HTTP {}",response.status()))}
    let body=response.json::<Value>().await.map_err(|e|e.to_string())?;
    let models=body.as_array().ok_or("모델 목록 형식 오류")?.iter()
        .filter_map(model_row).take(40).collect::<Vec<_>>();
    Ok(json!({"models":models,"query":q,"source":"huggingface"}))
}
pub async fn files(repo:&str)->Result<Value,String>{
    let client=client()?;
    let info=fetch_model(&client,repo).await?;
    if !model_is_video(&info,repo){return Err("영상 생성 모델 저장소만 다운로드할 수 있습니다.".into())}
    let mut url=hub_url(repo,&["tree","main"])?;
    url.query_pairs_mut().append_pair("recursive","1");
    let response=client.get(url).send().await.map_err(|e|e.to_string())?;
    if !response.status().is_success(){return Err(format!("파일 목록 조회 실패: HTTP {}",response.status()))}
    let body=response.json::<Value>().await.map_err(|e|e.to_string())?;
    let files=body.as_array().ok_or("모델 파일 목록 형식 오류")?.iter()
        .filter_map(|item|{
            let path=item["path"].as_str()?;
            let size=item["size"].as_u64()?;
            if !weight_path(path)||size<1024*1024||size>MAX_WEIGHT_BYTES{return None}
            Some(json!({"path":path,"size":size,
                "format":if path.ends_with(".gguf"){"GGUF"}else{"weights"},
                "quantization":quantization(path),
                "sha256":item["lfs"]["oid"].as_str().filter(|s|s.len()==64),
                "downloaded":false}))
        }).take(120).collect::<Vec<_>>();
    Ok(json!({"repo":repo,"files":files,"url":format!("https://huggingface.co/{repo}"),
        "license":info["cardData"]["license"],"gated":info["gated"],
        "runtime":"workflow_required"}))
}
fn quantization(path:&str)->String{
    let file=path.rsplit('/').next().unwrap_or(path).to_ascii_uppercase();
    for name in ["Q2_K","Q3_K_S","Q3_K_M","Q3_K_L","Q4_0","Q4_K_S",
        "Q4_K_M","Q5_0","Q5_K_S","Q5_K_M","Q6_K","Q8_0","FP8","FP16","BF16",
        "NVFP4","INT8"]{
        if file.contains(name){return name.to_string()}
    }
    "원본/기타".into()
}
fn stored_path(state:&AppState,repo:&str,path:&str)->Result<PathBuf,String>{
    let (owner,name)=repo_parts(repo)?;
    if !weight_path(path){return Err("가중치 파일 경로가 올바르지 않습니다.".into())}
    let mut root=state.config.data_dir.join("video-models").join("huggingface").join(owner).join(name);
    for component in path.split('/') {root.push(component);}
    Ok(root)
}
fn key_for(repo:&str,path:&str)->String{
    format!("video_model_download:hub-{:x}",Sha256::digest(format!("{repo}/{path}").as_bytes()))
}
pub async fn download(state:AppState,repo:&str,path:&str)->Result<Value,String>{
    if !weight_path(path){return Err("지원되는 모델 가중치 파일만 다운로드합니다.".into())}
    let listing=files(repo).await?;
    if listing["gated"]!=false&&!listing["gated"].is_null(){
        return Err("이 저장소는 접근 승인이 필요합니다. Hugging Face에서 라이선스를 확인하세요.".into())
    }
    let entry=listing["files"].as_array().and_then(|entries|entries.iter()
        .find(|entry|entry["path"]==path))
        .ok_or("저장소에 존재하는 영상 모델 가중치를 선택하세요.")?;
    let expected=entry["size"].as_u64().ok_or("모델 파일 크기가 없습니다.")?;
    let digest=entry["sha256"].as_str().map(str::to_owned);
    let target=stored_path(&state,repo,path)?;
    let key=key_for(repo,path);
    if std::fs::metadata(&target).is_ok_and(|meta|meta.is_file()&&meta.len()==expected){
        return Ok(json!({"id":key,"repo":repo,"path":path,"status":"completed",
            "bytes":expected,"total":expected,"progress":100}))
    }
    if state.store.get("video_model_download",&key)
        .is_some_and(|row|row["status"]=="downloading"){
        return Ok(json!({"id":key,"repo":repo,"path":path,"status":"downloading"}))
    }
    let record=json!({"id":key,"repo":repo,"path":path,"model":format!("{repo}/{path}"),
        "status":"downloading","bytes":0,"total":expected,"progress":0,
        "startedAt":chrono::Utc::now().to_rfc3339()});
    state.store.put("video_model_download",&record)
        .ok_or("영상 모델 다운로드 기록 저장 실패")?;
    let repo_for_worker=repo.to_owned();let file_path=path.to_owned();
    let state_for_worker=state.clone();let key_for_worker=key.clone();
    tokio::spawn(async move{
        let outcome=transfer(&state_for_worker,&key_for_worker,&repo_for_worker,&file_path,
            &target,expected,digest.as_deref()).await;
        let mut record=state_for_worker.store.get("video_model_download",&key_for_worker)
            .unwrap_or_else(||json!({"id":key_for_worker}));
        match outcome{
            Ok(())=>{record["status"]=json!("completed");record["progress"]=json!(100);
                record["bytes"]=json!(expected);}
            Err(error)=>{record["status"]=json!("failed");record["error"]=json!(error);}
        }
        record["finishedAt"]=json!(chrono::Utc::now().to_rfc3339());
        state_for_worker.store.put("video_model_download",&record);
    });
    Ok(json!({"id":key,"repo":repo,"path":path,"status":"downloading","total":expected}))
}
async fn transfer(state:&AppState,key:&str,repo:&str,path:&str,target:&Path,
                  expected:u64,sha256:Option<&str>)->Result<(),String>{
    let parent=target.parent().ok_or("저장 경로 오류")?;
    tokio::fs::create_dir_all(parent).await.map_err(|e|e.to_string())?;
    let part=target.with_extension(format!("{}{}",target.extension().and_then(|s|s.to_str())
        .unwrap_or("weight"),".part"));
    let mut existing=tokio::fs::metadata(&part).await.ok()
        .filter(|m|m.is_file()).map_or(0,|m|m.len());
    if existing>expected {tokio::fs::remove_file(&part).await.map_err(|e|e.to_string())?;existing=0;}
    let mut url=hub_url(repo,&["resolve","main"])?;
    url.path_segments_mut().map_err(|_|"다운로드 주소 오류")?.extend(path.split('/'));
    let client=reqwest::Client::builder().connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(12*3600)).build().map_err(|e|e.to_string())?;
    let mut request=client.get(url);
    if existing>0{request=request.header(reqwest::header::RANGE,format!("bytes={existing}-"));}
    let response=request.send().await.map_err(|e|e.to_string())?;
    let status=response.status();
    let resumed=existing>0&&status==reqwest::StatusCode::PARTIAL_CONTENT;
    if status==reqwest::StatusCode::PARTIAL_CONTENT{
        let range=response.headers().get(reqwest::header::CONTENT_RANGE)
            .and_then(|v|v.to_str().ok()).unwrap_or("");
        if !resumed||!range.starts_with(&format!("bytes {existing}-")){
            return Err("이어받기 범위가 맞지 않습니다.".into())
        }
    }
    if status==reqwest::StatusCode::RANGE_NOT_SATISFIABLE&&existing==expected {
        return verify_and_promote(&part,target,expected,sha256).await;
    }
    if !status.is_success(){return Err(format!("Hugging Face 다운로드 HTTP {status}"))}
    if !resumed {existing=0;}
    let mut file=tokio::fs::OpenOptions::new().create(true).write(true)
        .append(resumed).truncate(!resumed).open(&part).await.map_err(|e|e.to_string())?;
    use tokio::io::AsyncWriteExt;
    let mut received=existing;let mut since=0u64;
    let mut stream=response.bytes_stream();
    while let Some(chunk)=stream.next().await{
        let chunk=chunk.map_err(|e|e.to_string())?;
        received=received.checked_add(chunk.len() as u64).ok_or("파일 크기 초과")?;
        if received>expected{return Err("원본 파일보다 많은 바이트가 전송됐습니다.".into())}
        file.write_all(&chunk).await.map_err(|e|e.to_string())?;
        since+=chunk.len() as u64;
        if since>=8*1024*1024{
            since=0;
            let mut record=state.store.get("video_model_download",key).unwrap_or(json!({"id":key}));
            record["bytes"]=json!(received);record["progress"]=json!((received*100/expected).min(99));
            state.store.put("video_model_download",&record);
        }
    }
    file.flush().await.map_err(|e|e.to_string())?;
    file.sync_all().await.map_err(|e|e.to_string())?;drop(file);
    verify_and_promote(&part,target,expected,sha256).await
}
async fn verify_and_promote(part:&Path,target:&Path,expected:u64,sha256:Option<&str>)->Result<(),String>{
    let size=tokio::fs::metadata(part).await.map_err(|e|e.to_string())?.len();
    if size!=expected{return Err(format!("다운로드 불완전: {size}/{expected}바이트"))}
    if let Some(sha256)=sha256{
        let path=part.to_path_buf();
        let actual=tokio::task::spawn_blocking(move||->Result<String,String>{
            use std::io::Read;
            let mut file=std::fs::File::open(path).map_err(|e|e.to_string())?;
            let mut hasher=Sha256::new();
            let mut buffer=[0u8;1024*1024];
            loop {let n=file.read(&mut buffer).map_err(|e|e.to_string())?;
                if n==0{break}hasher.update(&buffer[..n]);}
            Ok(format!("{:x}",hasher.finalize()))
        }).await.map_err(|e|e.to_string())??;
        if actual!=sha256.to_ascii_lowercase(){return Err("원본 파일 SHA-256 검증 실패".into())}
    }
    if target.exists(){tokio::fs::remove_file(target).await.map_err(|e|e.to_string())?;}
    tokio::fs::rename(part,target).await.map_err(|e|e.to_string())
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]fn video_catalog_is_not_wan_locked(){
        assert!(FEATURED.iter().any(|(id,_)|id.contains("LTX")));
        assert!(FEATURED.iter().any(|(id,_)|id.contains("Hunyuan")));
        assert!(FEATURED.iter().any(|(id,_)|id.contains("CogVideo")));
        assert!(FEATURED.iter().any(|(id,_)|id.contains("mochi")));
        for (id,_) in FEATURED{assert!(repo_parts(id).is_ok());}
    }
    #[test]fn reject_path_traversal_code_and_nonvideo_model(){
        for path in ["../secret.gguf","nested/../evil.gguf","dir\\escape.gguf",
            "weights.bin","run.py",".hidden/model.gguf","bad%2Fpath.gguf"]{
            assert!(!weight_path(path),"{path}");
        }
        assert!(weight_path("split_files/diffusion_models/video-Q4_K_M.gguf"));
        assert!(weight_path("model.safetensors"));
        assert!(repo_parts("../evil").is_err());
        assert!(repo_parts("owner/../../evil").is_err());
        assert!(!model_is_video(&json!({"pipeline_tag":"text-generation","tags":["text-generation"]}),"Org/Llama-3"));
        assert!(model_is_video(&json!({"pipeline_tag":"text-to-video"}),"Org/SomeVideo"));
        assert_eq!(quantization("Wan-Q4_K_M.gguf"),"Q4_K_M");
    }
}
