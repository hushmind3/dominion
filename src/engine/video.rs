//! DOMINION's durable media pipeline. Native cards are rendered by Rust; FFmpeg
//! only encodes them. Optional ComfyUI is an explicit HTTP rendering connector.
use crate::{
    engine::{
        jobs::{self, JobContext},
        oauth, tools,
    },
    state::AppState,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

pub fn config(state: &AppState) -> Value {
    let mut value = json!({"id":"default","engine":"native","duration":15,"aspectRatio":"9:16","comfyUrl":"","comfyWorkflow":null,"workflowDir":"","outputDir":"","ffmpegPath":"","providerId":"","model":"","useModel":false,"narration":true,"subtitles":true,"voice":"Yuna","rate":170,"videoModel":"","resolution":"480x832","steps":20,"comfyModelsDir":""});
    if let Some(saved) = state.store.get("video_config", "default") {
        if let Some(map) = saved.as_object() {
            for (k, v) in map {
                if k != "videoApiKey" {
                    value[k] = v.clone();
                }
            }
        }
    }
    value["hasVideoKey"] = json!(state.store.has_secret("video:api"));
    value
}
/// Where ComfyUI keeps downloaded model files.
pub fn models_dir(state: &AppState) -> PathBuf {
    let cfg = config(state);
    match cfg["comfyModelsDir"].as_str().filter(|value| !value.is_empty()) {
        Some(path) => PathBuf::from(path),
        // HOME is commonly absent on Windows; a working-directory-relative
        // "models" folder made installed files appear missing after restart.
        // Keep explicit ComfyUI paths authoritative on every platform.
        None => std::env::var_os("HOME")
            .or_else(||std::env::var_os("USERPROFILE"))
            .map(|home|PathBuf::from(home).join("ComfyUI").join("models"))
            .unwrap_or_else(||state.config.data_dir.join("comfy-models")),
    }
}

fn present(state: &AppState, relative: &str) -> bool {
    models_dir(state).join(relative).metadata()
        .is_ok_and(|item|item.is_file()&&item.len()>1024*1024)
}

/// The download catalog contains VIDEO-GENERATION model variants only.
/// Text encoders and the VAE are dependencies bundled into each video model
/// install, not independent models exposed in the video model picker.
/// Sizes are approximate decimal GB, including shared companions.
/// Video generation families and their actual model variants.
/// Every entry has a manifest, and a ComfyUI API-format workflow below.
/// Download sizes include companion T5 and VAE files; shared assets are reused.
const VIDEO_T2V_VARIANTS:[(&str,&str,&str,&str,&str,f64,f64);13]=[
    ("wan21-t2v-1.3b-q3","wan21-t2v-1.3b","Q3_K_M","gguf",
        "unet/Wan2.1-T2V-1.3B-Q3_K_M.gguf",0.729,4.64),
    ("wan21-t2v-1.3b-q4","wan21-t2v-1.3b","Q4_K_M","gguf",
        "unet/Wan2.1-T2V-1.3B-Q4_K_M.gguf",0.983,4.90),
    ("wan21-t2v-1.3b-q5","wan21-t2v-1.3b","Q5_K_M","gguf",
        "unet/Wan2.1-T2V-1.3B-Q5_K_M.gguf",1.090,5.00),
    ("wan21-t2v-1.3b-q6","wan21-t2v-1.3b","Q6_K","gguf",
        "unet/Wan2.1-T2V-1.3B-Q6_K.gguf",1.200,5.11),
    ("wan21-t2v-1.3b-q8","wan21-t2v-1.3b","Q8_0","gguf",
        "unet/Wan2.1-T2V-1.3B-Q8_0.gguf",1.540,5.45),
    ("wan21-t2v-1.3b-fp16","wan21-t2v-1.3b","FP16","fp16",
        "diffusion_models/wan2.1_t2v_1.3B_fp16.safetensors",2.840,9.83),
    ("wan21-t2v-14b-q3","wan21-t2v-14b","Q3_K_M","gguf",
        "unet/wan2.1-t2v-14b-Q3_K_M.gguf",7.65,11.57),
    ("wan21-t2v-14b-q4","wan21-t2v-14b","Q4_K_M","gguf",
        "unet/wan2.1-t2v-14b-Q4_K_M.gguf",10.1,14.02),
    ("wan21-t2v-14b-q5","wan21-t2v-14b","Q5_K_M","gguf",
        "unet/wan2.1-t2v-14b-Q5_K_M.gguf",11.3,15.22),
    ("wan21-t2v-14b-q8","wan21-t2v-14b","Q8_0","gguf",
        "unet/wan2.1-t2v-14b-Q8_0.gguf",15.9,19.82),
    ("wan21-t2v-14b-fp8","wan21-t2v-14b","FP8","fp8",
        "diffusion_models/wan2.1_t2v_14B_fp8_scaled.safetensors",14.3,20.9),
    ("wan21-t2v-14b-fp16","wan21-t2v-14b","FP16","fp16",
        "diffusion_models/wan2.1_t2v_14B_fp16.safetensors",28.6,35.2),
    ("wan22-ti2v-5b-fp16","wan22-ti2v-5b","FP16","fp16",
        "diffusion_models/wan2.2_ti2v_5B_fp16.safetensors",10.0,17.7),
];
fn video_model_family(id:&str)->Option<&'static str>{
    VIDEO_T2V_VARIANTS.iter().find(|entry|entry.0==id).map(|entry|entry.1)
}
pub fn video_models(state:&AppState)->Vec<Value>{
    VIDEO_T2V_VARIANTS.iter().map(|(id,family,quant,kind,weight,weight_gb,total_gb)|{
        let (name,clip,vae)=match *family {
            "wan21-t2v-1.3b"=>(
                "Wan 2.1 · Text to Video · 1.3B",
                if *kind=="gguf"{"clip/umt5-xxl-encoder-Q4_K_M.gguf"}
                else{"text_encoders/umt5_xxl_fp8_e4m3fn_scaled.safetensors"},
                "vae/wan_2.1_vae.safetensors"),
            "wan21-t2v-14b"=>(
                "Wan 2.1 · Text to Video · 14B",
                if *kind=="gguf"{"clip/umt5-xxl-encoder-Q4_K_M.gguf"}
                else{"text_encoders/umt5_xxl_fp8_e4m3fn_scaled.safetensors"},
                "vae/wan_2.1_vae.safetensors"),
            _=>(
                "Wan 2.2 · Text/Image to Video · 5B",
                "text_encoders/umt5_xxl_fp8_e4m3fn_scaled.safetensors",
                "vae/wan2.2_vae.safetensors"),
        };
        let files=[*weight,clip,vae];
        let missing=files.iter().filter(|file|!present(state,file)).count();
        json!({
            "id":id,"familyId":family,"family":name,"task":"text-to-video",
            "name":format!("{name} · {quant}"),
            "quantization":quant,"kind":kind,"files":files,
            "weightGB":weight_gb,"totalDownloadGB":total_gb,
            "available":missing==0,"missingFiles":missing,
            "recommended":matches!(*id,
                "wan21-t2v-1.3b-q4"|"wan21-t2v-14b-q4"|"wan22-ti2v-5b-fp16"),
            "hint":if missing==0{"설치 완료".to_string()}else{
                format!("구성 파일 {missing}개 설치 필요")
            }
        })
    }).collect()
}

/// Workflow used when no imported workflow is set. Four-bit and fp16 loaders
/// differ only in the loader node; the sampler graph is identical.
pub fn workflow_for(model: &str, steps: u64) -> Option<Value> {
    let (unet, clip, vae, loader) = match model {
        "wan21-t2v-1.3b-q3" => ("Wan2.1-T2V-1.3B-Q3_K_M.gguf",
            "umt5-xxl-encoder-Q4_K_M.gguf","wan_2.1_vae.safetensors","gguf"),
        "wan21-t2v-1.3b-q4" => ("Wan2.1-T2V-1.3B-Q4_K_M.gguf",
            "umt5-xxl-encoder-Q4_K_M.gguf","wan_2.1_vae.safetensors","gguf"),
        "wan21-t2v-1.3b-q5" => ("Wan2.1-T2V-1.3B-Q5_K_M.gguf",
            "umt5-xxl-encoder-Q4_K_M.gguf","wan_2.1_vae.safetensors","gguf"),
        "wan21-t2v-1.3b-q6" => ("Wan2.1-T2V-1.3B-Q6_K.gguf",
            "umt5-xxl-encoder-Q4_K_M.gguf","wan_2.1_vae.safetensors","gguf"),
        "wan21-t2v-1.3b-q8" => ("Wan2.1-T2V-1.3B-Q8_0.gguf",
            "umt5-xxl-encoder-Q4_K_M.gguf","wan_2.1_vae.safetensors","gguf"),
        "wan21-t2v-1.3b-fp16" => (
            "wan2.1_t2v_1.3B_fp16.safetensors",
            "umt5_xxl_fp8_e4m3fn_scaled.safetensors",
            "wan_2.1_vae.safetensors",
            "fp16",
        ),
        "wan21-t2v-14b-q3" => ("wan2.1-t2v-14b-Q3_K_M.gguf",
            "umt5-xxl-encoder-Q4_K_M.gguf","wan_2.1_vae.safetensors","gguf"),
        "wan21-t2v-14b-q4" => ("wan2.1-t2v-14b-Q4_K_M.gguf",
            "umt5-xxl-encoder-Q4_K_M.gguf","wan_2.1_vae.safetensors","gguf"),
        "wan21-t2v-14b-q5" => ("wan2.1-t2v-14b-Q5_K_M.gguf",
            "umt5-xxl-encoder-Q4_K_M.gguf","wan_2.1_vae.safetensors","gguf"),
        "wan21-t2v-14b-q8" => ("wan2.1-t2v-14b-Q8_0.gguf",
            "umt5-xxl-encoder-Q4_K_M.gguf","wan_2.1_vae.safetensors","gguf"),
        "wan21-t2v-14b-fp8" => (
            "wan2.1_t2v_14B_fp8_scaled.safetensors",
            "umt5_xxl_fp8_e4m3fn_scaled.safetensors",
            "wan_2.1_vae.safetensors","fp8"),
        "wan21-t2v-14b-fp16" => (
            "wan2.1_t2v_14B_fp16.safetensors",
            "umt5_xxl_fp8_e4m3fn_scaled.safetensors",
            "wan_2.1_vae.safetensors","fp16"),
        "wan22-ti2v-5b-fp16" => (
            "wan2.2_ti2v_5B_fp16.safetensors",
            "umt5_xxl_fp8_e4m3fn_scaled.safetensors",
            "wan2.2_vae.safetensors","wan22"),
        _ => return None,
    };
    if loader=="wan22" {
        // Comfy-Org's published Wan 2.2 5B TI2V graph uses
        // Wan22ImageToVideoLatent and the 8-shift ModelSamplingSD3 node.
        return Some(json!({
            "1":{"class_type":"UNETLoader","inputs":{"unet_name":unet,"weight_dtype":"default"}},
            "2":{"class_type":"CLIPLoader","inputs":{"clip_name":clip,"type":"wan","device":"default"}},
            "3":{"class_type":"VAELoader","inputs":{"vae_name":vae}},
            "4":{"class_type":"CLIPTextEncode","inputs":{"text":"{{prompt}}","clip":["2",0]}},
            "5":{"class_type":"CLIPTextEncode","inputs":{"text":"{{negative}}","clip":["2",0]}},
            "6":{"class_type":"Wan22ImageToVideoLatent","inputs":{
                "vae":["3",0],"width":"{{width}}","height":"{{height}}",
                "length":"{{frames}}","batch_size":1}},
            "7":{"class_type":"ModelSamplingSD3","inputs":{"model":["1",0],"shift":8.0}},
            "8":{"class_type":"KSampler","inputs":{
                "model":["7",0],"positive":["4",0],"negative":["5",0],"latent_image":["6",0],
                "seed":"{{seed}}","steps":steps,"cfg":5.0,
                "sampler_name":"uni_pc","scheduler":"simple","denoise":1.0}},
            "9":{"class_type":"VAEDecode","inputs":{"samples":["8",0],"vae":["3",0]}},
            "10":{"class_type":"CreateVideo","inputs":{"images":["9",0],"fps":"{{fps}}"}},
            "11":{"class_type":"SaveVideo","inputs":{
                "video":["10",0],"filename_prefix":"dominion/wan22","format":"auto","codec":"auto"}}
        }));
    }
    let (unet_node, clip_node) = if loader == "gguf" {
        (json!({"class_type":"UnetLoaderGGUF","inputs":{"unet_name":unet}}),
         json!({"class_type":"CLIPLoaderGGUF","inputs":{"clip_name":clip,"type":"wan"}}))
    } else {
        (json!({"class_type":"UNETLoader","inputs":{"unet_name":unet,"weight_dtype":"default"}}),
         json!({"class_type":"CLIPLoader","inputs":{"clip_name":clip,"type":"wan"}}))
    };
    Some(json!({
        "1": unet_node,
        "2": clip_node,
        "3": {"class_type":"VAELoader","inputs":{"vae_name":vae}},
        "4": {"class_type":"CLIPTextEncode","inputs":{"text":"{{prompt}}","clip":["2",0]}},
        "5": {"class_type":"CLIPTextEncode","inputs":{"text":"{{negative}}","clip":["2",0]}},
        "6": {"class_type":"EmptyHunyuanLatentVideo","inputs":{
            "width":"{{width}}","height":"{{height}}","length":"{{frames}}","batch_size":1}},
        "7": {"class_type":"KSampler","inputs":{
            "model":["1",0],"positive":["4",0],"negative":["5",0],"latent_image":["6",0],
            "seed":"{{seed}}","steps":steps,"cfg":6.0,"sampler_name":"euler",
            "scheduler":"simple","denoise":1.0}},
        "8": {"class_type":"VAEDecode","inputs":{"samples":["7",0],"vae":["3",0]}},
        "9": {"class_type":"CreateVideo","inputs":{"images":["8",0],"fps":"{{fps}}"}},
        "10": {"class_type":"SaveVideo","inputs":{
            "video":["9",0],"filename_prefix":"dominion/wan","format":"mp4","codec":"h264"}}
    }))
}

/// All file URLs correspond to the selected VIDEO model package.
/// Shared T5/VAE assets are downloaded once; quantization variants reuse them.
pub fn model_files(id:&str)->Option<Vec<(&'static str,&'static str)>>{
    // Keep literal URLs so download mirrors are auditable.
    let (weight,url,fp16)=match id{
        "wan21-t2v-1.3b-q3"=>(
            "unet/Wan2.1-T2V-1.3B-Q3_K_M.gguf",
            concat!("https://huggingface.co/samuelchristlie/Wan2.1-T2V-1.3B-GGUF/resolve/main/",
                "Wan2.1-T2V-1.3B-Q3_K_M.gguf"),false),
        "wan21-t2v-1.3b-q4"=>(
            "unet/Wan2.1-T2V-1.3B-Q4_K_M.gguf",
            concat!("https://huggingface.co/samuelchristlie/Wan2.1-T2V-1.3B-GGUF/resolve/main/",
                "Wan2.1-T2V-1.3B-Q4_K_M.gguf"),false),
        "wan21-t2v-1.3b-q5"=>(
            "unet/Wan2.1-T2V-1.3B-Q5_K_M.gguf",
            concat!("https://huggingface.co/samuelchristlie/Wan2.1-T2V-1.3B-GGUF/resolve/main/",
                "Wan2.1-T2V-1.3B-Q5_K_M.gguf"),false),
        "wan21-t2v-1.3b-q6"=>(
            "unet/Wan2.1-T2V-1.3B-Q6_K.gguf",
            concat!("https://huggingface.co/samuelchristlie/Wan2.1-T2V-1.3B-GGUF/resolve/main/",
                "Wan2.1-T2V-1.3B-Q6_K.gguf"),false),
        "wan21-t2v-1.3b-q8"=>(
            "unet/Wan2.1-T2V-1.3B-Q8_0.gguf",
            concat!("https://huggingface.co/samuelchristlie/Wan2.1-T2V-1.3B-GGUF/resolve/main/",
                "Wan2.1-T2V-1.3B-Q8_0.gguf"),false),
        "wan21-t2v-1.3b-fp16"=>(
            "diffusion_models/wan2.1_t2v_1.3B_fp16.safetensors",
            "https://huggingface.co/Comfy-Org/Wan_2.1_ComfyUI_repackaged/resolve/main/split_files/diffusion_models/wan2.1_t2v_1.3B_fp16.safetensors",true),
        "wan21-t2v-14b-q3"=>(
            "unet/wan2.1-t2v-14b-Q3_K_M.gguf",
            "https://huggingface.co/city96/Wan2.1-T2V-14B-gguf/resolve/main/wan2.1-t2v-14b-Q3_K_M.gguf",false),
        "wan21-t2v-14b-q4"=>(
            "unet/wan2.1-t2v-14b-Q4_K_M.gguf",
            "https://huggingface.co/city96/Wan2.1-T2V-14B-gguf/resolve/main/wan2.1-t2v-14b-Q4_K_M.gguf",false),
        "wan21-t2v-14b-q5"=>(
            "unet/wan2.1-t2v-14b-Q5_K_M.gguf",
            "https://huggingface.co/city96/Wan2.1-T2V-14B-gguf/resolve/main/wan2.1-t2v-14b-Q5_K_M.gguf",false),
        "wan21-t2v-14b-q8"=>(
            "unet/wan2.1-t2v-14b-Q8_0.gguf",
            "https://huggingface.co/city96/Wan2.1-T2V-14B-gguf/resolve/main/wan2.1-t2v-14b-Q8_0.gguf",false),
        "wan21-t2v-14b-fp8"=>(
            "diffusion_models/wan2.1_t2v_14B_fp8_scaled.safetensors",
            "https://huggingface.co/Comfy-Org/Wan_2.1_ComfyUI_repackaged/resolve/main/split_files/diffusion_models/wan2.1_t2v_14B_fp8_scaled.safetensors",true),
        "wan21-t2v-14b-fp16"=>(
            "diffusion_models/wan2.1_t2v_14B_fp16.safetensors",
            "https://huggingface.co/Comfy-Org/Wan_2.1_ComfyUI_repackaged/resolve/main/split_files/diffusion_models/wan2.1_t2v_14B_fp16.safetensors",true),
        "wan22-ti2v-5b-fp16"=>(
            "diffusion_models/wan2.2_ti2v_5B_fp16.safetensors",
            "https://huggingface.co/Comfy-Org/Wan_2.2_ComfyUI_Repackaged/resolve/main/split_files/diffusion_models/wan2.2_ti2v_5B_fp16.safetensors",true),
        _=>return None,
    };
    let mut files=vec![(weight,url)];
    if fp16{
        files.push(("text_encoders/umt5_xxl_fp8_e4m3fn_scaled.safetensors",
            "https://huggingface.co/Comfy-Org/Wan_2.1_ComfyUI_repackaged/resolve/main/split_files/text_encoders/umt5_xxl_fp8_e4m3fn_scaled.safetensors"));
    }else{
        files.push(("clip/umt5-xxl-encoder-Q4_K_M.gguf",
            "https://huggingface.co/city96/umt5-xxl-encoder-gguf/resolve/main/umt5-xxl-encoder-Q4_K_M.gguf"));
    }
    if video_model_family(id)==Some("wan22-ti2v-5b"){
        files.push(("vae/wan2.2_vae.safetensors",
            "https://huggingface.co/Comfy-Org/Wan_2.2_ComfyUI_Repackaged/resolve/main/split_files/vae/wan2.2_vae.safetensors"));
    }else{
        files.push(("vae/wan_2.1_vae.safetensors",
            "https://huggingface.co/Comfy-Org/Wan_2.1_ComfyUI_repackaged/resolve/main/split_files/vae/wan_2.1_vae.safetensors"));
    }
    Some(files)
}

/// Real in-flight operations are process-local. Persisted "downloading"
/// receipts may survive a crash, but no task is running after restart.
fn active_model_downloads()->&'static std::sync::Mutex<std::collections::HashSet<String>>{
    static ACTIVE:std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<String>>>=
        std::sync::OnceLock::new();
    ACTIVE.get_or_init(||std::sync::Mutex::new(std::collections::HashSet::new()))
}
fn model_download_key(base:&Path,id:&str)->String {
    format!("{}::{id}",base.display())
}
struct DownloadGuard(String);
impl Drop for DownloadGuard {
    fn drop(&mut self){
        if let Ok(mut active)=active_model_downloads().lock(){
            active.remove(&self.0);
        }
    }
}
pub fn start_download(state:AppState,id:&str)->Result<Value,String>{
    let files=model_files(id).ok_or("알 수 없는 영상 모델입니다.")?;
    let key=format!("video_model_download:{id}");
    let base=models_dir(&state);
    let active_key=model_download_key(&base,id);
    {
        let mut active=active_model_downloads().lock()
            .map_err(|_|"모델 다운로드 상태를 읽지 못했습니다.")?;
        if !active.insert(active_key.clone()){
            return Err("이미 다운로드 중입니다.".into());
        }
    }
    let guard=DownloadGuard(active_key);
    if state.store.put("video_model_download",&json!({
        "id":key,"model":id,"status":"downloading","progress":0,
        "bytes":0,"total":0,"filesTotal":files.len(),
        "filesCompleted":0,"currentFile":null,
        "startedAt":chrono::Utc::now().to_rfc3339(),
    })).is_none(){
        return Err("다운로드 기록을 저장할 수 없습니다.".into());
    }
    let background_model_id=id.to_string();
    tokio::spawn(async move {
        let _guard=guard;
        let outcome=download_files(&state,&key,&base,&files).await;
        let mut record=state.store.get("video_model_download",&key)
            .unwrap_or_else(||json!({"id":key,"model":background_model_id}));
        match outcome {
            Ok(())=>{
                record["status"]=json!("completed");
                record["progress"]=json!(100);
                record["filesCompleted"]=json!(files.len());
                record["currentFile"]=Value::Null;
            }
            Err(error)=>{
                record["status"]=json!("failed");
                record["error"]=json!(error);
            }
        }
        record["finishedAt"]=json!(chrono::Utc::now().to_rfc3339());
        state.store.put("video_model_download",&record);
    });
    Ok(json!({"model":id,"status":"downloading"}))
}

/// Reports verified completed files and within-file progress. Summing only
/// known Content-Length headers used to show 99% after the first huge weight
/// while two more model files were still pending.
fn model_download_percent(completed:usize,total:usize,current_bytes:u64,current_total:u64)->u64 {
    if total==0 {return 0;}
    let partial=if current_total>0 {
        current_bytes.saturating_mul(100).saturating_div(current_total).min(99)
    }else{0};
    ((completed as u64).saturating_mul(100).saturating_add(partial)
        / total as u64).min(99)
}

async fn download_files(
    state: &AppState, key: &str, base: &Path, files: &[(&'static str, &'static str)],
) -> Result<(), String> {
    let client = reqwest::Client::builder().build().map_err(|e| e.to_string())?;
    let mut total_known = 0u64;
    let mut received = 0u64;
    let mut completed = 0usize;
    for (relative,url) in files {
        let target = base.join(relative);
        if target.metadata().is_ok_and(|m|m.is_file()&&m.len()>1024*1024) {
            completed+=1;
            if let Some(mut record)=state.store.get("video_model_download",key) {
                record["filesCompleted"]=json!(completed);
                record["progress"]=json!(model_download_percent(completed,files.len(),0,0));
                state.store.put("video_model_download",&record);
            }
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let response = client.get(*url).send().await.map_err(|e| e.to_string())?;
        if !response.status().is_success() {
            return Err(format!("다운로드 실패: HTTP {}", response.status()));
        }
        let this_total = response.content_length().unwrap_or(0);
        total_known=total_known.saturating_add(this_total);
        if let Some(mut record)=state.store.get("video_model_download",key) {
            record["currentFile"]=json!(relative);
            record["filesCompleted"]=json!(completed);
            record["bytes"]=json!(received);
            record["total"]=json!(total_known);
            record["progress"]=json!(model_download_percent(completed,files.len(),0,this_total));
            state.store.put("video_model_download",&record);
        }
        let part = target.with_extension("part");
        let mut file = tokio::fs::File::create(&part).await.map_err(|e| e.to_string())?;
        use futures_util::StreamExt;
        use tokio::io::AsyncWriteExt;
        let mut stream = response.bytes_stream();
        let mut current_received=0u64;
        let mut since_update=0u64;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| e.to_string())?;
            file.write_all(&chunk).await.map_err(|e| e.to_string())?;
            let n=chunk.len() as u64;
            received=received.saturating_add(n);
            current_received=current_received.saturating_add(n);
            since_update=since_update.saturating_add(n);
            if since_update >= 8 * 1024 * 1024 {
                since_update=0;
                if let Some(mut record)=state.store.get("video_model_download",key) {
                    record["bytes"]=json!(received);
                    record["total"]=json!(total_known.max(received));
                    record["filesCompleted"]=json!(completed);
                    record["progress"]=json!(model_download_percent(
                        completed,files.len(),current_received,this_total));
                    state.store.put("video_model_download",&record);
                }
            }
        }
        file.flush().await.map_err(|e| e.to_string())?;
        drop(file);
        if current_received<=1024*1024 || (this_total>0 && current_received!=this_total) {
            return Err(format!("모델 파일이 불완전합니다: {relative} ({current_received}/{this_total}바이트). 다시 다운로드해 주세요."));
        }
        tokio::fs::rename(&part,&target).await.map_err(|e| e.to_string())?;
        completed+=1;
        if let Some(mut record)=state.store.get("video_model_download",key) {
            record["filesCompleted"]=json!(completed);
            record["currentFile"]=Value::Null;
            record["bytes"]=json!(received);
            record["total"]=json!(total_known.max(received));
            record["progress"]=json!(model_download_percent(completed,files.len(),0,0));
            state.store.put("video_model_download",&record);
        }
    }
    Ok(())
}


pub fn downloads(state:&AppState)->Vec<Value>{
    let base=models_dir(state);
    state.store.all("video_model_download").into_iter().map(|mut row|{
        let interrupted=row["status"]=="downloading"
            && row["model"].as_str().is_some_and(|id|{
                let key=model_download_key(&base,id);
                active_model_downloads().lock().is_ok_and(|items|!items.contains(&key))
            });
        if interrupted {
            row["status"]=json!("interrupted");
            row["error"]=json!("다운로드가 중단되었습니다. 영상 제작에서 다시 받기를 눌러주세요.");
            state.store.put("video_model_download",&row);
        }
        row
    }).collect()
}

pub fn ffmpeg(state: &AppState) -> Option<PathBuf> {
    ffmpeg_from_config(&config(state))
}

/// Resolve the encoder from the job's frozen config so a later settings change
/// cannot redirect an already queued render.
fn ffmpeg_from_config(cfg: &Value) -> Option<PathBuf> {
    let selected = cfg["ffmpegPath"].as_str().unwrap_or("");
    if !selected.is_empty() {
        let path = PathBuf::from(selected);
        return path.is_file().then_some(path);
    }
    let name = if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    };
    let mut paths: Vec<PathBuf> =
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            .map(|p| p.join(name))
            .collect();
    paths.extend(
        [
            "/opt/homebrew/bin/ffmpeg",
            "/usr/local/bin/ffmpeg",
            "/usr/bin/ffmpeg",
        ]
        .into_iter()
        .map(PathBuf::from),
    );
    paths.into_iter().find(|p| p.is_file())
}
pub fn directory(state: &AppState, id: &str) -> Result<PathBuf, String> {
    // Job IDs are database-owned UUIDs, never arbitrary request paths.
    uuid::Uuid::parse_str(id).map_err(|_| "잘못된 영상 작업 ID")?;
    Ok(state.config.data_dir.join("video").join(id))
}
pub fn output_path(state: &AppState, id: &str) -> Result<PathBuf, String> {
    let record = state
        .store
        .get("job", id)
        .filter(|j| j["kind"] == "video")
        .ok_or("영상 작업이 없습니다.")?;
    if record["status"] != "completed" {
        return Err("완료된 영상이 없습니다.".into());
    }
    let root = directory(state, id)?
        .canonicalize()
        .map_err(|_| "영상 파일이 없습니다.")?;
    let path = root
        .join("output.mp4")
        .canonicalize()
        .map_err(|_| "영상 파일이 없습니다.")?;
    if !path.starts_with(root) {
        return Err("영상 경로를 확인할 수 없습니다.".into());
    }
    Ok(path)
}
/// A scene boundary is explicit: caller-supplied scene list or blank-line
/// separated paragraphs. Single-paragraph prompts retain the old workflow.
fn storyboard_scenes(body: &Value) -> Result<Vec<String>,String> {
    let scenes=if let Some(array)=body["scenes"].as_array(){
        array.iter().map(|item|item.as_str().unwrap_or("").trim().to_string())
            .collect::<Vec<_>>()
    }else{
        body["brief"].as_str().unwrap_or("")
            .split("\n\n").map(str::trim).filter(|text|!text.is_empty())
            .map(str::to_string).collect::<Vec<_>>()
    };
    if scenes.len()>8 || scenes.iter().any(|scene|
        scene.is_empty() || scene.len()>4000){
        return Err("장면은 최대 8개이며, 각 장면은 1~4000바이트여야 합니다.".into());
    }
    Ok(scenes)
}

fn scene_key(render:&Value,workflow:&Value,scene:&str,index:usize)->String {
    let contract=json!({"render":render,"workflow":workflow,"scene":scene,
        "index":index,"version":1});
    format!("{:x}",Sha256::digest(contract.to_string().as_bytes()))
}
/// Safely replace a finished video even on Windows, where rename cannot
/// overwrite an existing output. On failure the previously good MP4 remains.
fn install_video_output(part:&Path,destination:&Path)->Result<(),String>{
    if !part.metadata().is_ok_and(|file|file.is_file()&&file.len()>100){
        return Err("인코딩 완료된 영상 파일을 찾을 수 없습니다.".into());
    }
    if !destination.exists(){
        return std::fs::rename(part,destination).map_err(|e|e.to_string());
    }
    let backup=destination.with_extension("previous.mp4");
    if backup.exists(){
        std::fs::remove_file(&backup).map_err(|e|e.to_string())?;
    }
    std::fs::rename(destination,&backup).map_err(|e|e.to_string())?;
    if let Err(error)=std::fs::rename(part,destination){
        let rollback=std::fs::rename(&backup,destination);
        return Err(format!("영상 파일 교체 실패: {error}; 복구: {}",
            if rollback.is_ok(){"이전 영상 복원"}else{"백업 파일 보존"}));
    }
    let _=std::fs::remove_file(&backup);
    Ok(())
}

fn file_sha256(file:&Path)->Result<String,String>{
    use std::io::Read;
    // Video clips may be hundreds of MiB. Hash with a fixed-size buffer so
    // provenance checks do not allocate the full clip into process memory.
    let mut source=std::fs::File::open(file).map_err(|e|e.to_string())?;
    let mut digest=Sha256::new();
    let mut chunk=[0u8;64*1024];
    loop {
        let count=source.read(&mut chunk).map_err(|e|e.to_string())?;
        if count==0{break;}
        digest.update(&chunk[..count]);
    }
    Ok(format!("{:x}",digest.finalize()))
}

/// The estimate is model-agnostic pixel/frame work, not an invented provider
/// price. Scene reuse is only counted after manifest and SHA256 verification.
fn within_new_scene_budget(rendered:usize,max:Option<u64>)->bool {
    max.is_none_or(|limit|(rendered as u64)<limit)
}

fn render_workload(input: &Value) -> Value {
    let render = &input["render"];
    let frames = render["frames"].as_u64().unwrap_or(0);
    let pixels = render["width"].as_u64().unwrap_or(0)
        .saturating_mul(render["height"].as_u64().unwrap_or(0));
    let pixel_frames = pixels.saturating_mul(frames);
    json!({
        "frames":frames,
        "pixelFrames":pixel_frames,
        "megapixelFrames":pixel_frames / 1_000_000,
        "estimatedMoneyCost":Value::Null,
        "unit":"pixel_frames_not_currency",
    })
}

/// Frontend-safe durable video receipt. Keep the storyboard available even
/// before rendering begins; only expose playback for a completed job.
pub fn public_job(job: &Value) -> Value {
    let id = job["id"].as_str().unwrap_or("");
    let ready = job["kind"] == "video" && job["status"] == "completed";
    let render = &job["input"]["render"];
    let storyboard = job["videoState"]["scenes"].as_array()
        .filter(|items|!items.is_empty())
        .map(|items|json!(items))
        .unwrap_or_else(||job["input"]["scenes"].clone());
    json!({
        "id":id,"kind":job["kind"],"title":job["input"]["title"],
        "brief":job["input"]["brief"],"engine":job["input"]["engine"],
        "status":job["status"],
        "phase":job.get("phase").unwrap_or(&job["status"]),
        "progress":job["progress"],"error":job["error"],
        "createdAt":job.get("createdAt").or(job.get("created_at")),
        "videoJobId":job["input"]["videoJobId"],
        "privacyStatus":job["input"]["privacyStatus"],
        "outputUrl":ready.then(||format!("/api/video/jobs/{id}/content")),
        "storyboard":storyboard,"result":job["result"],
        "duration":job["input"]["duration"],
        "aspectRatio":job["input"]["aspectRatio"],
        "renderWorkload":render_workload(&job["input"]),
        "renderParams":{"fps":render["fps"],"seed":render["seed"],
            "width":render["width"],"height":render["height"]},
        "continuity":job["input"]["continuity"],
        "autoStoryboard":job["input"]["autoStoryboard"],
        "negativePrompt":render["negativePrompt"],
        "reuseVideoJobId":job["input"]["reuseVideoJobId"],
        "maxNewScenes":job["input"]["maxNewScenes"],
        "narrationScript":job["input"]["narrationScript"],
        "narrationLines":job["videoState"]["narrationLines"],
        "remoteCancel":job["remoteCancel"],
        "sceneProgress":{
            "index":job["videoState"]["sceneIndex"],
            "total":job["videoState"]["sceneTotal"],
            "rendered":job["videoState"]["renderedScenes"],
            "reused":job["videoState"]["reusedScenes"],
        }
    })
}
pub fn enqueue(state: &AppState, body: &Value) -> Result<Value, String> {
    let cfg = config(state);
    let brief = body["brief"].as_str().unwrap_or("").trim();
    if brief.is_empty() || brief.len() > 16_000 {
        return Err("영상 주제를 1~16,000바이트로 입력하세요.".into());
    }
    let engine = body["engine"]
        .as_str()
        .or(cfg["engine"].as_str())
        .unwrap_or("native");
    if !matches!(engine, "native" | "comfyui") {
        return Err("지원하지 않는 영상 엔진입니다.".into());
    }
    if engine == "native" && ffmpeg(state).is_none() {
        return Err("FFmpeg가 필요합니다. 고급 설정에서 실행 파일을 지정하세요.".into());
    }
    if engine == "comfyui" {
        let built_in = video_models(state).iter()
            .any(|model| model["id"] == cfg["videoModel"] && model["available"] == true);
        if cfg["comfyUrl"].as_str().unwrap_or("").is_empty()
            || (!cfg["comfyWorkflow"].is_object() && !built_in)
        {
            return Err("ComfyUI 주소와 영상 모델(또는 API 형식 워크플로 JSON)을 먼저 저장하세요.".into());
        }
    }
    let duration = body["duration"]
        .as_u64()
        .or(cfg["duration"].as_u64())
        .unwrap_or(15);
    if !(3..=180).contains(&duration) {
        return Err("영상 길이는 3~180초입니다.".into());
    }
    let scenes=storyboard_scenes(body)?;
    let auto_storyboard=body["autoStoryboard"]==true;
    if auto_storyboard && engine=="comfyui" && duration>=6 && ffmpeg(state).is_none() {
        return Err("자동 장면 기획으로 여러 장면을 합치려면 FFmpeg가 필요합니다.".into());
    }
    if auto_storyboard && !body["reuseVideoJobId"].as_str().unwrap_or("").is_empty() {
        return Err("기존 장면 재사용과 새 자동 스토리보드 생성은 함께 사용할 수 없습니다.".into());
    }
    if engine=="comfyui" {
        // Every scene needs at least three seconds. Reject impossible briefs
        // instead of silently outputting a longer video than was requested.
        if scenes.len()>1 && duration < (scenes.len() as u64).saturating_mul(3) {
            return Err("요청한 영상 길이가 장면 수에 비해 짧습니다. 장면당 최소 3초가 필요합니다.".into());
        }
    }
    let aspect = body["aspectRatio"]
        .as_str()
        .or(cfg["aspectRatio"].as_str())
        .unwrap_or("9:16");
    if !matches!(aspect, "9:16" | "16:9" | "1:1") {
        return Err("지원 비율은 9:16,16:9,1:1입니다.".into());
    }
    let title = body["title"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(brief)
        .chars()
        .take(90)
        .collect::<String>();
    let fps = body["fps"].as_u64().unwrap_or(16);
    let seed = body["seed"].as_i64().unwrap_or(-1);
    let negative_prompt = body["negativePrompt"].as_str().unwrap_or("");
    let narration_script = body["narrationScript"].as_str().unwrap_or("").trim();
    if narration_script.len()>8000 {
        return Err("내레이션 대본은 8000바이트 이하로 입력하세요.".into());
    }
    if !(1..=60).contains(&fps) || seed < -1 || negative_prompt.len() > 16_000 {
        return Err("FPS, 시드, 네거티브 프롬프트 값을 확인하세요.".into());
    }
    // A caller that picked a concrete resolution keeps it. Only the engine
    // that actually consumes width/height (ComfyUI) may override the ratio.
    let configured_size = cfg["resolution"].as_str()
        .and_then(|value| value.split_once('x'))
        .and_then(|(width, height)| Some((width.parse::<u64>().ok()?, height.parse::<u64>().ok()?)));
    let requested_size = if engine == "comfyui" {
        body["width"].as_u64().or(configured_size.map(|size| size.0))
            .zip(body["height"].as_u64().or(configured_size.map(|size| size.1)))
            .filter(|(width, height)| {
                (128..=1024).contains(width)
                    && (128..=1024).contains(height)
                    && width % 16 == 0
                    && height % 16 == 0
            })
    } else {
        None
    };
    let (width, height) = requested_size.unwrap_or_else(|| match aspect {
        "16:9" => (640, 360),
        "1:1" => (480, 480),
        _ => (360, 640),
    });
    let seed = if seed == -1 {
        // A partial regeneration reuses the previous project's frozen seed.
        // Otherwise fingerprints differ and every unchanged scene would
        // mistakenly be re-rendered at full GPU cost.
        body["reuseVideoJobId"].as_str()
            .and_then(|id|state.store.get("job",id))
            .and_then(|job|job["input"]["render"]["seed"].as_i64())
            .unwrap_or_else(||i64::from(u32::from_be_bytes(
                uuid::Uuid::new_v4().as_bytes()[..4].try_into().unwrap())))
    } else {
        seed
    };
    // A multi-scene ComfyUI project is completed with FFmpeg without
    // requiring any premium video API or a high-end user GPU.
    if engine=="comfyui" && scenes.len()>1 && ffmpeg(state).is_none(){
        return Err("여러 장면 결합에는 FFmpeg가 필요합니다.".into());
    }
    // Reject malformed or out-of-range regeneration selections BEFORE any
    // expensive remote video work. Never silently ignore an invalid scene.
    let regenerate=match body.get("regenerateScenes") {
        None | Some(Value::Null)=>vec![],
        Some(Value::Array(items))=>items.clone(),
        _=>return Err("재생성 장면은 0부터 시작하는 장면 번호 배열이어야 합니다.".into()),
    };
    let mut seen=std::collections::HashSet::new();
    for selected in &regenerate {
        let Some(index)=selected.as_u64() else {
            return Err("재생성 장면 번호는 음수가 아닌 정수여야 합니다.".into());
        };
        if index>=scenes.len() as u64 || !seen.insert(index) {
            return Err("재생성 장면 번호가 범위를 벗어나거나 중복되었습니다.".into());
        }
    }
    // Opt-in cap on NEW remote GPU renders. Reused SHA256-verified clips
    // are free to this quota; old jobs remain usable when this quota is zero.
    let max_new_scenes=match body.get("maxNewScenes") {
        None | Some(Value::Null)=>None,
        Some(value)=>Some(value.as_u64()
            .filter(|limit|*limit<=scenes.len() as u64)
            .ok_or("새 장면 최대 생성 횟수는 0~장면 수의 정수여야 합니다.")?),
    };
    if engine=="comfyui" && scenes.len()==1 && max_new_scenes==Some(0) {
        return Err("단일 장면 영상은 최소 1회의 새 렌더가 필요합니다.".into());
    }
    if let Some(old_id)=body["reuseVideoJobId"].as_str().filter(|v|!v.is_empty()){
        uuid::Uuid::parse_str(old_id).map_err(|_|"재사용할 영상 ID가 올바르지 않습니다.")?;
        if state.store.get("job",old_id)
            .is_none_or(|previous|previous["kind"]!="video"
                || previous["input"]["engine"]!="comfyui"){
            return Err("재사용할 기존 ComfyUI 영상 작업이 없습니다.".into());
        }
    }
    let input = json!({
        "brief":brief, "title":title, "engine":engine, "duration":duration,
        "aspectRatio":aspect, "config":cfg, "publish":false,
        "scenes":scenes,
        "autoStoryboard":auto_storyboard,
        "narrationScript":narration_script,
        "planningProviderId":body["planningProviderId"].as_str().unwrap_or(""),
        "continuity":body["continuity"].as_str().unwrap_or("").chars().take(2000).collect::<String>(),
        "reuseVideoJobId":body["reuseVideoJobId"].as_str().unwrap_or(""),
        "regenerateScenes":regenerate,
        "maxNewScenes":max_new_scenes,
        "render":{"prompt":brief,"negative":negative_prompt,"negativePrompt":negative_prompt,
                  "duration":duration,"width":width,"height":height,"fps":fps,"frames":duration*fps,"seed":seed}
    });
    jobs::enqueue(state, "video", input)
        .map(|j| public_job(&j))
        .ok_or("영상 작업을 저장할 수 없습니다.".into())
}
fn progress(ctx: &JobContext, phase: &str, percent: u64) -> Result<(), String> {
    ctx.checkpoint(json!({"phase":phase,"progress":percent}))
}
fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
fn wrap(s: &str, width: usize) -> Vec<String> {
    let mut lines = vec![];
    let mut line = String::new();
    let mut used = 0;
    for c in s.chars().take(280) {
        if c == '\n' || used >= width {
            lines.push(std::mem::take(&mut line));
            used = 0;
            if c == '\n' {
                continue;
            }
        }
        line.push(c);
        used += if c.is_ascii() { 1 } else { 2 };
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines.truncate(8);
    lines
}
fn make_cards(dir: &Path, title: &str, scenes: &[String], aspect: &str) -> Result<(), String> {
    let (w, h) = match aspect {
        "16:9" => (1280, 720),
        "1:1" => (900, 900),
        _ => (720, 1280),
    };
    let mut options = resvg::usvg::Options::default();
    options.fontdb_mut().load_system_fonts();
    let family = options
        .fontdb
        .faces()
        .find(|f| {
            f.families.iter().any(|(name, _)| {
                name.contains("Apple SD Gothic")
                    || name.contains("Malgun")
                    || name.contains("Noto Sans CJK")
            })
        })
        .and_then(|f| f.families.first())
        .map(|f| f.0.clone())
        .unwrap_or_else(|| "sans-serif".into());
    for (i, scene) in scenes.iter().enumerate() {
        let font = if aspect == "16:9" { 42 } else { 38 };
        let lines = wrap(scene, if aspect == "16:9" { 48 } else { 29 });
        let mut text = String::new();
        let top = (h as i32 / 2) - (lines.len() as i32 * (font + 18) / 2);
        for (n, line) in lines.iter().enumerate() {
            text.push_str(&format!("<text x='{}' y='{}' text-anchor='middle' fill='#fff9ef' font-size='{font}' font-family='{}'>{}</text>",w/2,top+n as i32*(font+18),escape(&family),escape(line)));
        }
        let heading = wrap(title, if aspect == "16:9" { 70 } else { 38 })
            .first()
            .cloned()
            .unwrap_or_default();
        let svg=format!("<svg xmlns='http://www.w3.org/2000/svg' width='{w}' height='{h}'><defs><linearGradient id='bg' x2='1' y2='1'><stop stop-color='#171f32'/><stop offset='1' stop-color='#314862'/></linearGradient></defs><rect width='{w}' height='{h}' fill='url(#bg)'/><circle cx='{}' cy='{}' r='240' fill='#c96442' opacity='.15'/><rect x='48' y='86' width='80' height='5' rx='2' fill='#edb78e'/><text x='48' y='140' font-family='{}' font-size='22' fill='#edb78e'>{}</text>{text}<text x='48' y='{}' font-family='sans-serif' font-size='20' fill='#d0d9e4'>{:02} / {:02}</text><rect x='48' y='{}' width='{}' height='4' fill='#edb78e'/></svg>",w,h,escape(&family),escape(&heading),h-55,i+1,scenes.len(),h-30,(w-96)*(i+1)as u32/scenes.len()as u32);
        let tree = resvg::usvg::Tree::from_str(&svg, &options).map_err(|e| e.to_string())?;
        let mut bitmap = resvg::tiny_skia::Pixmap::new(w, h).ok_or("영상 프레임 메모리 부족")?;
        resvg::render(
            &tree,
            resvg::tiny_skia::Transform::default(),
            &mut bitmap.as_mut(),
        );
        bitmap
            .save_png(dir.join(format!("card-{i:02}.png")))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
/// Platform-native offline narration. This keeps creation usable without
/// purchasing a TTS API: macOS uses say, Windows uses installed SAPI voices.
/// Missing speech support is an explicit silent-video fallback.
async fn say_scenes(
    dir: &Path, scenes: &[String], cfg: &Value, encoder: &Path,
) -> Option<(Vec<f64>, std::path::PathBuf)> {
    if cfg["narration"] == false { return None; }
    let voice = cfg["voice"].as_str().filter(|value| !value.is_empty()).unwrap_or("Yuna");
    let rate = cfg["rate"].as_u64().unwrap_or(170).clamp(80, 320).to_string();
    let ffprobe = encoder.with_file_name(if cfg!(windows) {"ffprobe.exe"} else {"ffprobe"});
    let mut durations = Vec::new();
    let mut list = String::new();
    for (index, scene) in scenes.iter().enumerate() {
        let file = format!("narration-{index:02}.{}",if cfg!(windows){"wav"}else{"aiff"});
        let path = dir.join(&file);
        #[cfg(target_os="macos")]
        let status = tokio::time::timeout(Duration::from_secs(45),
            tokio::process::Command::new("say")
                .args(["-v", voice, "-r", &rate, "-o"])
                .arg(&path).arg("--").arg(scene).status()
        ).await.ok()?.ok()?;
        #[cfg(target_os="windows")]
        let status = {
            // Voice and scene text enter through environment variables, never
            // as PowerShell source. A missing Yuna voice falls back to the
            // Windows default installed SAPI voice.
            let script=r#"Add-Type -AssemblyName System.Speech; $s = New-Object System.Speech.Synthesis.SpeechSynthesizer; $v = $env:DOMINION_TTS_VOICE; if ($v) { try { $s.SelectVoice($v) } catch {} }; $s.Rate = [int]$env:DOMINION_TTS_RATE; $s.SetOutputToWaveFile($env:DOMINION_TTS_PATH); $s.Speak($env:DOMINION_TTS_TEXT); $s.Dispose()"#;
            let sapi_rate=((rate.parse::<i32>().unwrap_or(170)-170)/20).clamp(-7,7);
            tokio::time::timeout(Duration::from_secs(45),
                tokio::process::Command::new("powershell.exe")
                    .args(["-NoProfile","-NonInteractive","-Command",script])
                    .env("DOMINION_TTS_VOICE",voice)
                    .env("DOMINION_TTS_RATE",sapi_rate.to_string())
                    .env("DOMINION_TTS_PATH",&path)
                    .env("DOMINION_TTS_TEXT",scene).status()
            ).await.ok()?.ok()?
        };
        #[cfg(not(any(target_os="macos",target_os="windows")))]
        let status:std::process::ExitStatus=return None;
        if !status.success() { return None; }
        let seconds = probe_duration(&ffprobe, &path).await?;
        // Some installed "voices" are novelty entries that render silence;
        // a near-zero file would make -shortest truncate the whole video.
        if seconds < 0.2 { return None; }
        durations.push((seconds + 0.35).max(1.0));
        list.push_str(&format!("file '{file}'\n"));
    }
    std::fs::write(dir.join("narration.txt"), list).ok()?;
    let status = tokio::process::Command::new(encoder)
        .current_dir(dir)
        .args(["-hide_banner","-loglevel","error","-y","-f","concat","-safe","0",
            "-i","narration.txt","-c:a","aac","-b:a","128k","narration.m4a"])
        .status()
        .await
        .ok()?;
    if !status.success() { return None; }
    Some((durations, dir.join("narration.m4a")))
}

/// These subtitles are real muxed MP4 mov_text tracks, not fake UI labels.
fn srt_stamp(ms:u64)->String {
    let s=ms/1000;
    format!("{:02}:{:02}:{:02},{:03}",s/3600,(s/60)%60,s%60,ms%1000)
}
fn captions_from_narration(input:&Value,script:&str)->String {
    let duration_ms=input["duration"].as_u64().unwrap_or(6).saturating_mul(1000);
    let mut lines=input["narrationLines"].as_array()
        .map(|items|items.iter().filter_map(Value::as_str)
            .map(str::trim).filter(|s|!s.is_empty()).map(str::to_owned)
            .collect::<Vec<_>>()).unwrap_or_default();
    if lines.is_empty(){
        lines=script.split(['\n','。','!','?','.'])
            .map(str::trim).filter(|s|!s.is_empty())
            .map(str::to_owned).collect();
    }
    if lines.is_empty(){lines.push(script.trim().to_owned());}
    let count=lines.len() as u64;
    let mut out=String::new();
    for (index,line) in lines.iter().enumerate(){
        // Each cue takes its own time slice, with no overlapping boundaries.
        let start=duration_ms.saturating_mul(index as u64)/count;
        let end=duration_ms.saturating_mul(index as u64+1)/count;
        out.push_str(&format!("{}\n{} --> {}\n{}\n\n",
            index+1,srt_stamp(start),srt_stamp(end),line));
    }
    out
}

/// A complete AI-rendered clip is independently useful even when the host
/// lacks a speech voice. Optional narration/captions use FFmpeg offline and
/// atomically replace the old video only after the output validates.
async fn postprocess_narration(ctx:&JobContext,input:&Value,dir:&Path)->Result<Value,String>{
    let config=&input["config"];
    let explicit=input["narrationScript"].as_str().unwrap_or("").trim();
    let planned=input["narrationLines"].as_array()
        .map(|lines|lines.iter().filter_map(Value::as_str)
            .filter(|s|!s.trim().is_empty()).collect::<Vec<_>>().join(" "))
        .unwrap_or_default();
    let script=if explicit.is_empty(){planned.as_str()}else{explicit};
    if script.trim().is_empty() {
        return Ok(json!({"narrated":false,"captions":false}));
    }
    ctx.checkpoint(json!({"phase":"audio","progress":92}))?;
    let ffmpeg=ffmpeg_from_config(config)
        .ok_or("내레이션과 자막 합성에는 FFmpeg가 필요합니다.")?;
    let audio=if config["narration"]==false{None}
        else{say_scenes(dir,&[script.to_string()],config,&ffmpeg).await};
    let add_captions=config["subtitles"]!=false;
    if !add_captions && audio.is_none(){
        return Ok(json!({"narrated":false,"captions":false,
            "narrationWarning":"음성 엔진이 없어 소리를 생성하지 못했습니다."}));
    }
    if add_captions {
        std::fs::write(dir.join("captions.srt"),captions_from_narration(input,script))
            .map_err(|e|e.to_string())?;
    }
    let mut cmd=tokio::process::Command::new(&ffmpeg);
    cmd.current_dir(dir).args(["-hide_banner","-loglevel","error","-nostdin",
        "-y","-i","output.mp4"]);
    if audio.is_some(){cmd.args(["-i","narration.m4a"]);}
    if add_captions{cmd.args(["-f","srt","-i","captions.srt"]);}
    cmd.args(["-map","0:v:0","-c:v","copy"]);
    if audio.is_some() {
        cmd.args(["-map","1:a:0","-c:a","aac","-af","apad"]);
    }else{
        cmd.args(["-map","0:a?","-c:a","copy"]);
    }
    if add_captions {
        let subtitle_map=if audio.is_some(){"2:s:0"}else{"1:s:0"};
        cmd.args(["-map",subtitle_map,"-c:s","mov_text"]);
    }
    cmd.args(["-shortest","-movflags","+faststart","output.narrated.part.mp4"]);
    let process=tools::run_command(&mut cmd,Duration::from_secs(180));
    tokio::pin!(process);
    let output=loop {
        tokio::select! {
            result=&mut process=>break result?,
            _=tokio::time::sleep(Duration::from_millis(500))=>{
                ctx.checkpoint(json!({}))?;
            }
        }
    };
    if !output.status.success(){
        return Err(format!("영상 음성·자막 합성 실패: {}",
            String::from_utf8_lossy(&output.stderr).chars().take(600).collect::<String>()));
    }
    install_video_output(&dir.join("output.narrated.part.mp4"),&dir.join("output.mp4"))?;
    let warning=if audio.is_none()&&config["narration"]!=false {
        "이 환경에서 음성 합성이 지원되지 않아 자막만 추가했습니다."
    }else{""};
    Ok(json!({"narrated":audio.is_some(),"captions":add_captions,
        "narrationWarning":warning}))
}

async fn probe_duration(ffprobe: &Path, file: &Path) -> Option<f64> {
    let output = tokio::process::Command::new(ffprobe)
        .args(["-v","error","-show_entries","format=duration","-of","default=nw=1:nk=1"])
        .arg(file)
        .output()
        .await
        .ok()?;
    String::from_utf8_lossy(&output.stdout).trim().parse().ok()
}

async fn native(ctx: &JobContext, input: &Value, dir: &Path) -> Result<Value, String> {
    progress(ctx, "storyboard", 5)?;
    let brief = input["brief"].as_str().unwrap_or("");
    let title = input["title"].as_str().unwrap_or("");
    let cfg = &input["config"];
    let mut scenes: Vec<String> = if input["autoStoryboard"]==true {
        input["scenes"].as_array().map(|items|items.iter()
            .filter_map(|s|s.as_str().map(str::to_string)).collect())
            .unwrap_or_default()
    } else {
        brief.split(['\n', '.', '!', '?', '。']).map(str::trim)
            .filter(|s|!s.is_empty()).take(12).map(str::to_string).collect()
    };
    if cfg["useModel"]==true && input["autoStoryboard"]!=true {
        let text=crate::engine::llm::complete(&ctx.state,cfg["providerId"].as_str().unwrap_or(""),cfg["model"].as_str().unwrap_or(""),"영상 자막 스토리보드를 만드세요. JSON 문자열 배열만 반환하세요. 3~8개 장면, 장면당 한국어 100자 이내. 사실을 지어내지 마세요.",brief).await?;
        scenes = serde_json::from_str::<Vec<String>>(
            text.trim()
                .trim_start_matches("```json")
                .trim_start_matches("```")
                .trim_end_matches("```")
                .trim(),
        )
        .map_err(|_| "모델 스토리보드가 JSON 문자열 배열이 아닙니다.")?;
        if scenes.is_empty() || scenes.len() > 12 || scenes.iter().any(|s| s.len() > 2000) {
            return Err("스토리보드 장면 수나 길이가 범위를 넘었습니다.".into());
        }
    }
    if scenes.is_empty() {
        scenes.push(brief.into());
    }
    ctx.checkpoint(json!({"videoState":{"scenes":scenes},"phase":"rendering","progress":20}))?;
    make_cards(
        dir,
        title,
        &scenes,
        input["aspectRatio"].as_str().unwrap_or("9:16"),
    )?;
    let encoder = ffmpeg_from_config(&input["config"]).ok_or("FFmpeg를 찾을 수 없습니다.")?;
    let requested = input["duration"].as_u64().unwrap_or(15);
    // Turnkey: speak every scene first. Fall back to a silent slideshow when
    // no voice engine is available, never failing the whole render for audio.
    let narration = say_scenes(dir, &scenes, &input["config"], &encoder).await;
    let mut concat = String::new();
    let total_seconds;
    if let Some((durations, _)) = &narration {
        for (index, seconds) in durations.iter().enumerate() {
            concat.push_str(&format!("file 'card-{index:02}.png'\nduration {seconds:.5}\n"));
        }
        concat.push_str(&format!("file 'card-{:02}.png'\n", scenes.len() - 1));
        total_seconds = durations.iter().sum::<f64>();
    } else {
        let seconds = requested as f64 / scenes.len() as f64;
        for index in 0..scenes.len() {
            concat.push_str(&format!("file 'card-{index:02}.png'\nduration {seconds:.5}\n"));
        }
        concat.push_str(&format!("file 'card-{:02}.png'\n", scenes.len() - 1));
        total_seconds = requested as f64;
    }
    std::fs::write(dir.join("frames.txt"), concat).map_err(|e| e.to_string())?;
    std::fs::write(
        dir.join("storyboard.json"),
        json!({"title":title,"scenes":scenes,"duration":total_seconds.round() as u64,
            "audio":if narration.is_some() {"narration"} else {"silent"}}).to_string(),
    )
    .map_err(|e| e.to_string())?;
    let mut command = tokio::process::Command::new(&encoder);
    command.current_dir(dir).args([
        "-hide_banner","-loglevel","error","-nostdin","-y","-f","concat","-safe","1","-i","frames.txt",
    ]);
    if narration.is_some() {
        command.args(["-i","narration.m4a","-map","0:v:0","-map","1:a:0"]);
    } else {
        command.args(["-f","lavfi","-i","anullsrc=r=48000:cl=stereo"]);
    }
    command.args([
        "-t",&format!("{:.3}", total_seconds + 0.2),
        "-vf","fps=24,format=yuv420p","-c:v","libx264","-preset","veryfast","-crf","22",
        "-c:a","aac","-movflags","+faststart","-shortest","output.part.mp4",
    ]);
    progress(ctx, "encoding", 50)?;
    let process = tools::run_command(&mut command, Duration::from_secs(600));
    tokio::pin!(process);
    let output = loop {
        tokio::select! {result=&mut process=>break result?,_ = tokio::time::sleep(Duration::from_millis(500))=>{ctx.checkpoint(json!({}))?;}}
    };
    if !output.status.success() {
        return Err(format!(
            "FFmpeg 인코딩 실패: {}",
            String::from_utf8_lossy(&output.stderr).chars().take(1500).collect::<String>()
        ));
    }
    install_video_output(&dir.join("output.part.mp4"),&dir.join("output.mp4"))?;
    Ok(json!({
        "engine":"native",
        "description":if narration.is_some() {"한국어 내레이션 카드 영상"} else {"Rust 자막 카드 영상 · 무음"},
        "duration":total_seconds.round() as u64,
        "scenes":scenes.len(),
        "narrated":narration.is_some(),
    }))
}

fn substitute(value: &mut Value, variables: &Value) {
    match value {
        Value::String(text) => {
            if let Some(replacement) = text.strip_prefix("{{").and_then(|s| s.strip_suffix("}}"))
                .and_then(|key| variables.get(key)) {
                *value = replacement.clone();
                return;
            }
            let mut expanded = String::new();
            let mut rest = text.as_str();
            while let Some((prefix, candidate)) = rest.split_once("{{") {
                let Some((key, suffix)) = candidate.split_once("}}") else { break };
                expanded.push_str(prefix);
                match variables.get(key) {
                    Some(Value::String(replacement)) => expanded.push_str(replacement),
                    Some(replacement) => expanded.push_str(&replacement.to_string()),
                    None => { expanded.push_str("{{"); expanded.push_str(key); expanded.push_str("}}"); }
                }
                rest = suffix;
            }
            expanded.push_str(rest);
            *text = expanded;
        }
        Value::Array(items) => items.iter_mut().for_each(|item| substitute(item, variables)),
        Value::Object(fields) => fields.values_mut().for_each(|item| substitute(item, variables)),
        _ => {}
    }
}
/// Shared native Rust cancellation: API and Rust SDK call the same
/// durable state machine. Remote renderers receive only prompt-scoped IDs.
/// Acknowledgement is not treated as proof that GPU execution has stopped.
fn preferable_cancel_receipt(existing:&Value,proposed:&Value)->Value {
    // A later generic/unconfirmed observation cannot erase a scoped remote
    // cancel request already acknowledged during the submission race.
    if existing["status"]=="requested"{return existing.clone();}
    if proposed["status"]=="requested"{return proposed.clone();}
    if existing["status"]=="unconfirmed"
        && (proposed["status"]=="not_active" || proposed["promptId"].is_null()){
        return existing.clone();
    }
    proposed.clone()
}

pub async fn cancel_video(state:&AppState,id:&str)->Result<Value,jobs::CancelJobError>{
    let job=state.store.get("job",id)
        .filter(|record|record["kind"]=="video")
        .ok_or(jobs::CancelJobError::NotFound)?;
    if !matches!(job["status"].as_str(),Some("queued"|"running")){
        return Err(jobs::CancelJobError::Conflict);
    }
    let in_flight=job["videoState"]["inFlight"]==true;
    let prompt_id=job["videoState"]["promptId"].as_str()
        .and_then(|id|uuid::Uuid::parse_str(id).ok()).map(|id|id.to_string());
    let base=job["input"]["config"]["comfyUrl"].as_str().unwrap_or("").to_string();
    jobs::cancel_job(state,id)?;
    let remote=if job["input"]["engine"]!="comfyui" || !in_flight{
        json!({"status":"not_active"})
    }else if let Some(prompt_id)=prompt_id{
        match cancel_comfy_prompt(&base,&prompt_id).await{
            Ok(true)=>json!({"status":"requested","promptId":prompt_id}),
            Ok(false)=>json!({"status":"unconfirmed","promptId":prompt_id,
                "reason":"원격 작업이 종료됐거나 서버에서 해당 작업을 찾지 못했습니다."}),
            Err(reason)=>json!({"status":"unconfirmed","promptId":prompt_id,"reason":reason}),
        }
    }else{
        // Submission response has no prompt ID yet. Worker will cancel the
        // exact returned ID when the /prompt request completes.
        json!({"status":"unconfirmed",
            "reason":"원격 제출 응답 대기 중입니다. Rust 작업자가 ID 확인 후 개별 취소를 시도합니다."})
    };
    // CAS on the receipt field, not only terminal status. A worker can finish
    // its /prompt response and record an acknowledged remote cancellation
    // at the same moment this HTTP/native SDK stop finishes.
    let mut saved=None;
    for _ in 0..3 {
        let current=state.store.get("job",id)
            .ok_or(jobs::CancelJobError::NotFound)?;
        let accepted=preferable_cancel_receipt(&current["remoteCancel"],&remote);
        if accepted==current["remoteCancel"] {
            saved=Some(current);
            break;
        }
        if let Some(updated)=state.store.patch_if("job",id,"remoteCancel",
            &current["remoteCancel"],&json!({"remoteCancel":accepted})){
            saved=Some(updated);
            break;
        }
    }
    let saved=saved.or_else(||state.store.get("job",id))
        .ok_or(jobs::CancelJobError::Conflict)?;
    let receipt=saved["remoteCancel"].clone();
    state.store.audit("video.cancel",json!({"jobId":id,"remoteCancel":receipt}));
    state.bus.record("job",&saved);
    Ok(json!({"job":public_job(&saved),"remoteCancel":receipt}))
}

/// Cancel only the prompt ID produced by this DOMINION job. Never call
/// ComfyUI's global /interrupt: that would abort unrelated user workloads.
/// Modern ComfyUI implements this idempotent, prompt-scoped endpoint.
pub async fn cancel_comfy_prompt(base: &str, prompt_id: &str) -> Result<bool, String> {
    uuid::Uuid::parse_str(prompt_id).map_err(|_| "원격 렌더 작업 ID가 올바르지 않습니다.")?;
    let base = base.trim_end_matches('/');
    let url = reqwest::Url::parse(base).map_err(|_| "ComfyUI 주소 오류")?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("ComfyUI HTTP(S) 주소를 확인하세요.".into());
    }
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(6))
        .build().map_err(|e| e.to_string())?;
    let response = client.post(format!("{base}/api/jobs/{prompt_id}/cancel"))
        .send().await.map_err(|e| format!("원격 취소 응답을 받지 못했습니다: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("원격 취소를 확인하지 못했습니다: HTTP {}", response.status()));
    }
    let receipt: Value = response.json().await
        .map_err(|_| "원격 취소 확인 응답이 유효하지 않습니다.")?;
    receipt["cancelled"].as_bool()
        .ok_or_else(|| "원격 취소 확인 값이 없습니다.".to_string())
}

/// Accept legacy /history/{id} and modern /api/jobs/{id} receipts without
/// mistaking a queued job for completion or a failed job for a missing output.
fn comfy_result_status(result: &Value) -> &'static str {
    match result["status"].as_str().or_else(||result["status"]["status_str"].as_str()) {
        Some("completed"|"success")=>"completed",
        Some("error"|"failed")=>"failed",
        Some("cancelled"|"canceled")=>"cancelled",
        Some("pending"|"in_progress"|"running"|"queued")=>"running",
        _ if result["status"]["completed"]==true=>"completed",
        _=>"running",
    }
}

async fn comfy(ctx: &JobContext, input: &Value, dir: &Path) -> Result<Value, String> {
    let cfg = &input["config"];
    let base = cfg["comfyUrl"].as_str().unwrap_or("").trim_end_matches('/');
    let parsed = reqwest::Url::parse(base).map_err(|_| "ComfyUI 주소 오류")?;
    if !matches!(parsed.scheme(), "http" | "https")
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return Err("ComfyUI HTTP(S) 주소를 확인하세요.".into());
    }
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| e.to_string())?;
    let mut workflow = if cfg["comfyWorkflow"].is_object() {
        cfg["comfyWorkflow"].clone()
    } else {
        let steps = cfg["steps"].as_u64().unwrap_or(20).clamp(1, 60);
        workflow_for(cfg["videoModel"].as_str().unwrap_or(""), steps)
            .ok_or("영상 모델을 설정에서 선택하세요.")?
    };
    let legacy = json!({"prompt":input["brief"],"duration":input["duration"]});
    substitute(&mut workflow, input.get("render").unwrap_or(&legacy));
    ctx.checkpoint(json!({"phase":"submitting","progress":10,
        "videoState":{"inFlight":true,"promptId":null}}))?;
    let response = client
        .post(format!("{base}/prompt"))
        .json(&json!({"prompt":workflow,"client_id":ctx.job_id}))
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .map_err(|_| "needs_review: ComfyUI 제출 결과를 확인할 수 없습니다.")?;
    if !response.status().is_success() {
        ctx.checkpoint(json!({"videoState":{"inFlight":false}}))?;
        return Err(format!("ComfyUI 워크플로 거부: HTTP {}", response.status()));
    }
    let answer: Value = response
        .json()
        .await
        .map_err(|_| "needs_review: ComfyUI 제출 응답 오류")?;
    let prompt_id = answer["prompt_id"]
        .as_str()
        .ok_or("needs_review: ComfyUI prompt_id가 없습니다.")?;
    uuid::Uuid::parse_str(prompt_id)
        .map_err(|_|"needs_review: ComfyUI prompt_id 형식 오류")?;
    // The user can cancel while /prompt is still returning. If cancellation
    // wins before the remote ID was journaled, stop the newly submitted prompt
    // by its exact ID rather than leaving an orphan GPU render.
    if let Err(error) = ctx.checkpoint(json!({
        "phase":"rendering","progress":25,
        "videoState":{"inFlight":true,"promptId":prompt_id}
    })) {
        let attempt = cancel_comfy_prompt(base, prompt_id).await;
        let remote = match &attempt {
            Ok(true) => json!({"status":"requested","promptId":prompt_id}),
            Ok(false) => json!({"status":"unconfirmed","promptId":prompt_id,
                "reason":"원격 작업이 종료되었거나 서버가 작업을 찾지 못했습니다."}),
            Err(reason) => json!({"status":"unconfirmed","promptId":prompt_id,"reason":reason}),
        };
        if let Some(record) = ctx.state.store.patch_if(
            "job", &ctx.job_id, "status", &json!("cancelled"),
            &json!({"remoteCancel":remote})
        ) {
            ctx.state.bus.record("job", &record);
        }
        ctx.state.store.audit("video.remote_cancel_after_submit",
            json!({"jobId":ctx.job_id,"remoteCancel":remote}));
        return Err(error);
    }
    let start = std::time::Instant::now();
    loop {
        ctx.checkpoint(json!({}))?;
        if start.elapsed() > Duration::from_secs(900) {
            return Err(
                "needs_review: ComfyUI 생성 대기시간이 지났습니다. 원격 작업 상태를 확인하세요."
                    .into(),
            );
        }
        let response = client
            .get(format!("{base}/history/{prompt_id}"))
            .timeout(Duration::from_secs(10))
            .send().await
            .map_err(|e|format!("needs_review: ComfyUI 조회 실패: {e}"))?;
        let modern = response.status()==reqwest::StatusCode::NOT_FOUND;
        let response = if modern {
            // Current ComfyUI also serves detailed job receipts here.
            // No extra POST is issued: a polling fallback never re-renders.
            client.get(format!("{base}/api/jobs/{prompt_id}"))
                .timeout(Duration::from_secs(10)).send().await
                .map_err(|e|format!("needs_review: ComfyUI 작업 조회 실패: {e}"))?
        } else {response};
        if response.status()==reqwest::StatusCode::NOT_FOUND {
            tokio::time::sleep(Duration::from_secs(2)).await;
            continue; // Submitted work can still be waiting for its history row.
        }
        if !response.status().is_success() {
            return Err(format!("needs_review: ComfyUI 작업 상태 HTTP {}",response.status()));
        }
        let history:Value=response.json().await
            .map_err(|_|"needs_review: ComfyUI 작업 상태 응답 오류")?;
        let result = if modern {&history} else {&history[prompt_id]};
        let status=comfy_result_status(result);
        if matches!(status,"failed"|"cancelled") {
            ctx.checkpoint(json!({"videoState":{"inFlight":false,"promptId":prompt_id,
                "remoteStatus":status}}))?;
            return Err(format!("ComfyUI 작업이 {status} 상태로 종료됐습니다."));
        }
        if let Some(outputs) = result["outputs"].as_object() {
            for node in outputs.values() {
                // SaveVideo reports mp4 files under "images" with animated=true.
                for field in ["videos", "gifs", "images"] {
                    if let Some(files) = node[field].as_array() {
                        for file in files {
                            let filename = file["filename"].as_str().unwrap_or("");
                            if !filename.to_lowercase().ends_with(".mp4") {
                                continue;
                            }
                            let response = client
                                .get(format!("{base}/view"))
                                .query(&[
                                    ("filename", filename),
                                    ("subfolder", file["subfolder"].as_str().unwrap_or("")),
                                    ("type", file["type"].as_str().unwrap_or("output")),
                                ])
                                .timeout(Duration::from_secs(120))
                                .send()
                                .await
                                .map_err(|e| e.to_string())?;
                            if !response.status().is_success() {
                                return Err("ComfyUI 결과 다운로드 실패".into());
                            }
                            use futures_util::StreamExt;
                            use tokio::io::AsyncWriteExt;
                            let mut stream = response.bytes_stream();
                            let mut total = 0usize;
                            let mut output = tokio::fs::File::create(dir.join("output.part.mp4"))
                                .await
                                .map_err(|e| e.to_string())?;
                            while let Some(chunk) = stream.next().await {
                                ctx.checkpoint(json!({}))?;
                                let chunk = chunk.map_err(|e| e.to_string())?;
                                total += chunk.len();
                                if total > 256 * 1024 * 1024 {
                                    return Err("영상 결과가 256MiB 제한을 넘었습니다.".into());
                                }
                                output.write_all(&chunk).await.map_err(|e| e.to_string())?;
                            }
                            drop(output);
                            install_video_output(&dir.join("output.part.mp4"),&dir.join("output.mp4"))?;
                            ctx.checkpoint(
                                json!({"videoState":{"inFlight":false,"promptId":prompt_id}}),
                            )?;
                            return Ok(
                                json!({"engine":"comfyui","promptId":prompt_id,"bytes":total}),
                            );
                        }
                    }
                }
            }
            if status=="completed" {
                return Err(
                    "ComfyUI 완료 결과에 MP4가 없습니다. 영상 출력 노드를 확인하세요.".into(),
                );
            }
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}
/// Read a completed clip only when the saved manifest and its bytes match the
/// *current* frozen render input. This is valid for both prior projects and
/// the same job after an interrupted, safely re-queued execution.
fn verified_scene_clip(project: &Path, index: usize, fingerprint: &str) -> bool {
    let scene_dir = project.join(format!("scene-{index:02}"));
    let clip = scene_dir.join("output.mp4");
    let proof = std::fs::read_to_string(scene_dir.join("manifest.json"))
        .ok().and_then(|text| serde_json::from_str::<Value>(&text).ok());
    let contained = clip.canonicalize().ok().zip(project.canonicalize().ok())
        .is_some_and(|(source, root)| source.starts_with(root));
    let Some(proof) = proof else { return false; };
    contained
        && proof["fingerprint"].as_str() == Some(fingerprint)
        && proof["sceneIndex"].as_u64() == Some(index as u64)
        && proof["engine"] == "comfyui"
        && clip.metadata().is_ok_and(|file| file.is_file() && file.len() > 100)
        && file_sha256(&clip).ok().as_deref() == proof["sha256"].as_str()
}

/// Keep each generated clip and its input fingerprint. A failed or cancelled
/// project can reuse verified, unchanged clips in a fresh job rather than
/// paying for the same GPU work again. Assembly uses local FFmpeg.
async fn comfy_storyboard(ctx:&JobContext,input:&Value,root:&Path)->Result<Value,String>{
    let scenes=input["scenes"].as_array().ok_or("장면 목록이 없습니다.")?;
    ctx.checkpoint(json!({"videoState":{"scenes":scenes,"sceneTotal":scenes.len()},
        "phase":"storyboard","progress":5}))?;
    let encoder=ffmpeg_from_config(&input["config"])
        .ok_or("다중 장면 영상에 FFmpeg가 필요합니다.")?;
    let mut index_file=String::new();
    let mut reused=0usize;
    let mut resumed=0usize;
    let mut reused_frames=0u64;
    let mut rendered=0usize;
    let mut render_seconds=0.0_f64;
    let previous=input["reuseVideoJobId"].as_str().filter(|id|!id.is_empty())
        .and_then(|id|directory(&ctx.state,id).ok());
    let force=input["regenerateScenes"].as_array().cloned().unwrap_or_default();
    let total=scenes.len();
    for (index,scene) in scenes.iter().enumerate(){
        ctx.checkpoint(json!({"phase":"rendering",
            "progress":(10+index*65/total) as u64,
            "videoState":{"sceneIndex":index,"sceneTotal":total,
                "renderedScenes":rendered,"reusedScenes":reused}}))?;
        let text=scene.as_str().ok_or("장면 텍스트가 없습니다.")?;
        let scene_dir=root.join(format!("scene-{index:02}"));
        std::fs::create_dir_all(&scene_dir).map_err(|e|e.to_string())?;
        // Distribute remainders without inflating the requested total length.
        // enqueue already checked duration >= 3 * scene_count.
        let requested=input["duration"].as_u64().unwrap_or(15);
        let seconds=requested/(total as u64)+u64::from((index as u64)<requested%(total as u64));
        let mut scene_input=input.clone();
        scene_input["brief"]=json!(text);
        scene_input["duration"]=json!(seconds);
        let continuity=input["continuity"].as_str().unwrap_or("").trim();
        let prompt=if continuity.is_empty(){text.to_string()}else{
            format!("Consistent identity and visual style: {continuity}. Scene: {text}")
        };
        scene_input["render"]["prompt"]=json!(prompt);
        scene_input["render"]["duration"]=json!(seconds);
        let fps=scene_input["render"]["fps"].as_u64().unwrap_or(16);
        // Freeze a distinct, reproducible seed for every independent scene.
        let base_seed=input["render"]["seed"].as_i64().unwrap_or(0);
        scene_input["render"]["seed"]=json!(base_seed.saturating_add(index as i64));
        scene_input["render"]["frames"]=json!(seconds.saturating_mul(fps));
        let config=&scene_input["config"];
        let workflow=if config["comfyWorkflow"].is_object(){
            config["comfyWorkflow"].clone()
        }else{
            workflow_for(config["videoModel"].as_str().unwrap_or(""),
                config["steps"].as_u64().unwrap_or(20).clamp(1,60))
                .ok_or("영상 모델이 설정되지 않았습니다.")?
        };
        let signature=scene_key(&scene_input["render"],&workflow,text,index);
        let target=scene_dir.join("output.mp4");
        let manifest=scene_dir.join("manifest.json");
        let forced=force.iter().any(|value|value.as_u64()==Some(index as u64));
        // Recovered queued jobs keep their original output directory. Reusing
        // their own already verified clips avoids re-paying for finished scenes.
        // A forced render from an earlier attempt is still safe to reuse
        // from THIS job once its new manifest is committed; only old jobs
        // must ignore the selected force-regenerate scene.
        let mut used_previous=false;
        if verified_scene_clip(root,index,&signature) {
            used_previous=true;
            resumed+=1;
        } else if !forced {
            if let Some(old_root)=previous.as_ref() {
                if verified_scene_clip(old_root,index,&signature) {
                    let source_dir=old_root.join(format!("scene-{index:02}"));
                    std::fs::copy(source_dir.join("output.mp4"),&target)
                        .map_err(|e|e.to_string())?;
                    std::fs::copy(source_dir.join("manifest.json"),&manifest)
                        .map_err(|e|e.to_string())?;
                    used_previous=true;
                }
            }
        }
        if used_previous {
            reused+=1;
            reused_frames=reused_frames.saturating_add(seconds.saturating_mul(fps));
        } else {
            if !within_new_scene_budget(rendered,input["maxNewScenes"].as_u64()) {
                return Err(format!("새 장면 렌더 예산을 초과했습니다: {rendered}개 완료, 다음 장면 {index}. 기존 장면은 재사용할 수 있습니다."));
            }
            let started=std::time::Instant::now();
            comfy(ctx,&scene_input,&scene_dir).await?;
            render_seconds+=started.elapsed().as_secs_f64();
            if target.metadata().map_err(|e|e.to_string())?.len()<=100 {
                return Err("생성된 개별 장면이 비어 있습니다.".into());
            }
            // Save the durable, content-addressed provenance after successful
            // download; never trust an unfinished mp4 from a crashed task.
            let proof=json!({"fingerprint":signature,"sha256":file_sha256(&target)?,
                "sceneIndex":index,"seconds":seconds,"engine":"comfyui"});
            std::fs::write(&manifest,proof.to_string()).map_err(|e|e.to_string())?;
            rendered+=1;
        }
        index_file.push_str(&format!("file 'scene-{index:02}/output.mp4'\n"));
        ctx.checkpoint(json!({"phase":"rendering",
            "progress":(10+(index+1)*65/total) as u64,
            "videoState":{"sceneIndex":index+1,"sceneTotal":total,
                "renderedScenes":rendered,"reusedScenes":reused}}))?;
    }
    std::fs::write(root.join("scene-concat.txt"),index_file)
        .map_err(|e|e.to_string())?;
    progress(ctx,"assembling",82)?;
    let mut command=tokio::process::Command::new(&encoder);
    command.current_dir(root).args(["-hide_banner","-loglevel","error",
        "-nostdin","-y","-f","concat","-safe","1","-i","scene-concat.txt",
        "-c","copy","-movflags","+faststart","output.part.mp4"]);
    let output=tools::run_command(&mut command,Duration::from_secs(600)).await?;
    if !output.status.success(){
        // Input clips can differ in codec metadata. Re-encode locally rather
        // than discarding already generated scenes and paying for them again.
        let mut fallback=tokio::process::Command::new(&encoder);
        fallback.current_dir(root).args(["-hide_banner","-loglevel","error",
            "-nostdin","-y","-f","concat","-safe","1","-i","scene-concat.txt",
            "-c:v","libx264","-preset","veryfast","-crf","22",
            "-c:a","aac","-movflags","+faststart","output.part.mp4"]);
        let reencoded=tools::run_command(&mut fallback,Duration::from_secs(600)).await?;
        if !reencoded.status.success(){
            return Err(format!("장면 병합 실패: {}",
                String::from_utf8_lossy(&reencoded.stderr)
                    .chars().take(1000).collect::<String>()));
        }
    }
    install_video_output(&root.join("output.part.mp4"),&root.join("output.mp4"))?;
    progress(ctx,"quality_check",96)?;
    let actual_duration=probe_duration(&encoder.with_file_name("ffprobe"),
        &root.join("output.mp4")).await;
    let size=std::fs::metadata(root.join("output.mp4"))
        .map_err(|e|e.to_string())?.len();
    if size<=100 || actual_duration.is_some_and(|seconds|seconds<=0.1){
        return Err("완성 영상의 크기 또는 길이가 유효하지 않습니다.".into());
    }
    Ok(json!({"engine":"comfyui","mode":"storyboard",
        "sceneCount":total,"renderedScenes":rendered,"reusedScenes":reused,
        "resumedScenes":resumed,"renderSeconds":render_seconds,
        "generatedFrames":input["render"]["fps"].as_u64().unwrap_or(16)
            .saturating_mul(input["duration"].as_u64().unwrap_or(0))
            .saturating_sub(reused_frames),
        "reusedFrames":reused_frames,
        "duration":actual_duration,
        "provenance":"content-addressed segment manifests",
        "assembly":"local ffmpeg concat"}))
}

fn parse_video_storyboard(text:&str,limit:usize)->Result<Vec<String>,String>{
    let text=text.trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    let scenes:Vec<String>=serde_json::from_str(text)
        .map_err(|_|"장면 기획 모델이 JSON 문자열 배열을 반환하지 않았습니다.")?;
    if scenes.len()<2 || scenes.len()>limit
        || scenes.iter().any(|s|s.trim().is_empty()||s.len()>4000)
        || scenes.join("\n\n").len()>16000 {
        return Err("자동 스토리보드 장면 수 또는 내용 길이가 유효하지 않습니다.".into());
    }
    Ok(scenes.into_iter().map(|s|s.trim().to_string()).collect())
}

/// Structured one-call scene plan: visuals and concise spoken narration
/// share a frozen boundary. Legacy JSON string arrays remain supported.
fn parse_video_plan(text:&str,limit:usize)->Result<(Vec<String>,Vec<String>),String>{
    let clean=text.trim().trim_start_matches("```json")
        .trim_start_matches("```").trim_end_matches("```").trim();
    let value:Value=serde_json::from_str(clean)
        .map_err(|_|"AI 장면 계획은 JSON이어야 합니다.")?;
    if value.is_array(){
        return Ok((parse_video_storyboard(clean,limit)?,vec![]));
    }
    let objects=value["scenes"].as_array()
        .ok_or("AI 장면 계획에 scenes 배열이 없습니다.")?;
    let visuals=objects.iter()
        .map(|scene|scene["visual"].as_str().unwrap_or("").trim().to_owned())
        .collect::<Vec<_>>();
    let visuals=parse_video_storyboard(&json!(visuals).to_string(),limit)?;
    let narration=objects.iter().map(|scene|
        scene["narration"].as_str().unwrap_or("").trim().to_owned()).collect::<Vec<_>>();
    if narration.iter().any(|line|line.len()>600)
        || narration.iter().any(|line|line.is_empty()){
        return Err("AI 내레이션 대본이 누락됐거나 장면당 600바이트를 넘었습니다.".into());
    }
    Ok((visuals,narration))
}

async fn plan_video_scenes(ctx:&JobContext,input:&Value)->Result<Option<(Vec<String>,Vec<String>)>,String>{
    if input["autoStoryboard"]!=true{return Ok(None);}
    let duration=input["duration"].as_u64().unwrap_or(6);
    let max_by_time=(duration/3).clamp(1,4) as usize;
    let limit=input["maxNewScenes"].as_u64().unwrap_or(4)
        .min(max_by_time as u64) as usize;
    if limit<2{return Ok(None);}
    ctx.checkpoint(json!({"phase":"storyboard","progress":3}))?;
    let preferred=input["planningProviderId"].as_str().unwrap_or("");
    let route=crate::engine::gateway::resolve(&ctx.state,preferred,None)
        .map_err(|_|"자동 대본 기획에 사용할 텍스트 AI 제공자를 먼저 연결하세요.")?;
    let system=format!(
        "당신은 영상 감독이자 내레이터입니다. 2~{limit}장면을 구성하세요. \
         JSON 객체 하나만 반환: {{\"scenes\":[{{\"visual\":\"구체적인 영상 생성 지시문\",\"narration\":\"해당 장면의 짧은 한국어 내레이션\"}}]}}. \
         visual에는 같은 인물·의상·시각 스타일·카메라 움직임을 적고 4000바이트 미만으로 하세요. \
         narration은 장면 길이 3초마다 약 12~20글자의 자연스러운 말투로 짧게 작성하세요. \
         제공되지 않은 사실을 지어내지 말고 장면 전환을 이어주세요."
    );
    let brief=input["brief"].as_str().unwrap_or("");
    let text=crate::engine::llm::complete(&ctx.state,&route.id,&route.model,
        &system,brief).await?;
    let (scenes,narration)=parse_video_plan(&text,limit)?;
    ctx.checkpoint(json!({
        "phase":"storyboard","progress":9,
        "videoState":{"scenes":scenes,"sceneTotal":scenes.len(),
            "narrationLines":narration}
    }))?;
    Ok(Some((scenes,narration)))
}

pub async fn run(ctx: &JobContext, job: &Value) -> Result<Value, String> {
    let dir = directory(&ctx.state, &ctx.job_id)?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut input = job["input"].clone();
    // Recover a previously frozen LLM plan before any further paid model call.
    // A malformed saved plan is an explicit failure, not permission to silently
    // generate a different plan and invalidate all scene fingerprints.
    if input["autoStoryboard"] == true && job["videoState"]["scenes"].is_array() {
        let saved=parse_video_storyboard(&job["videoState"]["scenes"].to_string(),4)?;
        input["scenes"]=json!(saved);
        input["narrationLines"]=job["videoState"]["narrationLines"].clone();
    } else if let Some((planned,narration))=plan_video_scenes(ctx,&input).await? {
        input["scenes"]=json!(planned);
        input["narrationLines"]=json!(narration);
    }
    let input=&input;
    let mut result = if input["engine"] == "comfyui" {
        if input["scenes"].as_array().is_some_and(|items|items.len()>1){
            comfy_storyboard(ctx,input,&dir).await
        }else{
            comfy(ctx, input, &dir).await
        }
    } else {
        native(ctx, input, &dir).await
    };
    if result.is_ok() && input["engine"] == "comfyui" {
        match postprocess_narration(ctx,input,&dir).await {
            Ok(post)=>if let Ok(value)=result.as_mut(){
                for key in ["narrated","captions","narrationWarning"] {
                    if !post[key].is_null(){value[key]=post[key].clone();}
                }
            },
            Err(message)=>{
                if ctx.state.store.get("job",&ctx.job_id)
                    .is_some_and(|job|job["cancelRequested"]==true){
                    result=Err(message);
                }else if let Ok(value)=result.as_mut(){
                    // A failed optional audio pass never destroys the rendered MP4.
                    value["narrationWarning"]=json!(message);
                }
            }
        }
    }
    match result {
        Ok(mut value) => {
            let bytes = std::fs::metadata(dir.join("output.mp4"))
                .map_err(|e| e.to_string())?
                .len();
            if bytes < 100 {
                return Err("생성된 영상이 비어 있습니다.".into());
            }
            value["bytes"] = json!(bytes);
            value["outputUrl"] = json!(format!("/api/video/jobs/{}/content", ctx.job_id));
            progress(ctx, "completed", 100)?;
            Ok(value)
        }
        Err(e) => {
            let _ = std::fs::remove_file(dir.join("output.part.mp4"));
            if ctx
                .state
                .store
                .get("job", &ctx.job_id)
                .is_some_and(|j| j["videoState"]["inFlight"] == true)
                && !e.starts_with("needs_review:")
            {
                Err(format!("needs_review: {e}"))
            } else {
                Err(e)
            }
        }
    }
}
pub fn enqueue_upload(state: &AppState, id: &str, body: &Value) -> Result<Value, String> {
    output_path(state, id)?;
    let video = state.store.get("job", id).ok_or("영상이 없습니다.")?;
    if oauth::access_token(state, "youtube").is_none() {
        return Err("연결 메뉴에서 YouTube 업로드 계정을 먼저 연결하세요.".into());
    }
    let privacy = body["privacyStatus"].as_str().unwrap_or("private");
    if !matches!(privacy, "private" | "unlisted" | "public") {
        return Err("공개 범위 오류".into());
    }
    use sha2::Digest;
    let digest = sha2::Sha256::digest(format!("dominion-youtube:{id}"));
    let mut uuid = [0u8; 16];
    uuid.copy_from_slice(&digest[..16]);
    let upload_id = uuid::Uuid::from_bytes(uuid).to_string();
    let made_for_kids = body["madeForKids"].as_bool().unwrap_or(false);
    let title = body["title"].as_str().or_else(|| video["input"]["title"].as_str())
        .unwrap_or_default().trim();
    let description = body["description"].as_str().or_else(|| video["input"]["brief"].as_str())
        .unwrap_or_default();
    let tags = body["tags"].as_str().unwrap_or("").split(',')
        .map(str::trim).filter(|tag| !tag.is_empty()).collect::<Vec<_>>();
    if title.is_empty() || title.chars().count() > 100 || description.len() > 5000
        || tags.join(",").chars().count() > 500 {
        return Err("YouTube 제목, 설명, 태그 길이를 확인하세요.".into());
    }
    let candidate = json!({
        "id":upload_id,"kind":"youtube_upload","status":"queued",
        "queuedAt":chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros,true),
        "input":{"videoJobId":id,"privacyStatus":privacy,"madeForKids":made_for_kids,
                 "title":title,"description":description,"tags":tags},
        "attempts":0,"stepIndex":0,"stepTotal":0,"events":[]
    });
    let job = if let Some(inserted) = state.store.put_if_absent("job", &candidate) {
        state.bus.record("job", &inserted);
        state.store.audit(
            "job.queued",
            json!({"kind":"youtube_upload","jobId":upload_id}),
        );
        inserted
    } else {
        let existing = state
            .store
            .get("job", &upload_id)
            .ok_or("업로드 작업 조회 실패")?;
        if existing["status"] == "failed" && existing["videoState"]["inFlight"] != true {
            state.store.patch_if("job",&upload_id,"status",&json!("failed"),&json!({"status":"queued","error":null,"attempts":0,"queuedAt":chrono::Utc::now().to_rfc3339(),"input":candidate["input"]})).unwrap_or(existing)
        } else {
            existing
        }
    };
    Ok(public_job(&job))
}
pub async fn upload(ctx: &JobContext, job: &Value) -> Result<Value, String> {
    if let Some(result) = job
        .pointer("/videoState/result")
        .filter(|r| r["youtubeId"].as_str().is_some_and(|s| !s.is_empty()))
    {
        return Ok(result.clone());
    }
    if job["videoState"]["inFlight"] == true {
        return Err("needs_review: 이전 업로드 결과를 먼저 확인하세요.".into());
    }
    let id = job["input"]["videoJobId"]
        .as_str()
        .ok_or("영상 작업 ID 필요")?;
    let path = output_path(&ctx.state, id)?;
    let video = ctx.state.store.get("job", id).ok_or("영상이 없습니다.")?;
    let token = oauth::fresh_access_token(&ctx.state, "youtube").await?;
    let size = tokio::fs::metadata(&path)
        .await
        .map_err(|e| e.to_string())?
        .len();
    if size > 256 * 1024 * 1024 {
        return Err("업로드 크기 제한은 256MiB입니다.".into());
    }
    let bytes = tokio::fs::read(path).await.map_err(|e| e.to_string())?;
    if bytes.len() as u64 != size {
        return Err("업로드 중 파일 크기가 변경되었습니다. 다시 시도하세요.".into());
    }
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| e.to_string())?;
    progress(ctx, "authorizing", 5)?;
    let title = job["input"]["title"].as_str().or_else(|| video["input"]["title"].as_str());
    let description = job["input"]["description"].as_str().or_else(|| video["input"]["brief"].as_str());
    let tags = job["input"]["tags"].as_array().cloned().unwrap_or_default();
    let response=client.post("https://www.googleapis.com/upload/youtube/v3/videos").query(&[("uploadType","resumable"),("part","snippet,status")]).bearer_auth(&token).header("X-Upload-Content-Length",bytes.len()).header("X-Upload-Content-Type","video/mp4").json(&json!({"snippet":{"title":title,"description":description,"tags":tags},"status":{"privacyStatus":job["input"]["privacyStatus"],"selfDeclaredMadeForKids":job["input"]["madeForKids"].as_bool().unwrap_or(false)}})).timeout(Duration::from_secs(30)).send().await.map_err(|_|"YouTube 업로드 세션 연결 실패")?;
    if !response.status().is_success() {
        return Err(format!(
            "YouTube 업로드 세션 거부: HTTP {}",
            response.status()
        ));
    }
    let location = response
        .headers()
        .get("location")
        .and_then(|v| v.to_str().ok())
        .ok_or("YouTube 업로드 주소가 없습니다.")?;
    let url = reqwest::Url::parse(location).map_err(|_| "YouTube 업로드 주소 오류")?;
    if url.scheme() != "https"
        || !matches!(
            url.host_str(),
            Some("www.googleapis.com" | "upload.youtube.com")
        )
    {
        return Err("YouTube가 반환한 업로드 호스트를 확인할 수 없습니다.".into());
    }
    ctx.checkpoint(json!({"phase":"uploading","progress":20,"videoState":{"inFlight":true}}))?;
    // Persist the opaque session URL encrypted. A crash after PUT is uncertain:
    // it is never automatically submitted again and cannot create duplicates.
    ctx.state
        .save_secret(&format!("youtube-upload:{}", ctx.job_id), location);
    let response = client
        .put(url)
        .bearer_auth(token)
        .header("Content-Type", "video/mp4")
        .header("Content-Length", bytes.len())
        .body(bytes)
        .timeout(Duration::from_secs(600))
        .send()
        .await
        .map_err(|_| {
            "needs_review: YouTube 업로드 결과가 불확실합니다. 업로드 기록을 확인하세요."
        })?;
    if !response.status().is_success() {
        return Err(format!(
            "needs_review: YouTube 업로드 응답 HTTP {}. 원격 상태를 확인하세요.",
            response.status()
        ));
    }
    let value: Value = response
        .json()
        .await
        .map_err(|_| "needs_review: YouTube 업로드 결과 해석 실패")?;
    let youtube_id = value["id"]
        .as_str()
        .filter(|s| {
            s.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        })
        .ok_or("needs_review: YouTube 영상 ID 없음")?;
    let result = json!({"youtubeId":youtube_id,"url":format!("https://www.youtube.com/watch?v={youtube_id}"),"privacyStatus":job["input"]["privacyStatus"]});
    ctx.checkpoint(
        json!({"phase":"completed","progress":100,"videoState":{"inFlight":false,"result":result}}),
    )?;
    ctx.state
        .store
        .remove_secret(&format!("youtube-upload:{}", ctx.job_id));
    Ok(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frozen_ffmpeg_path_wins_over_current_settings() {
        let dir = std::env::temp_dir().join(format!("dominion-ffmpeg-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let fake = dir.join("ffmpeg-custom");
        std::fs::write(&fake, b"binary").unwrap();
        let cfg = json!({"ffmpegPath": fake.to_string_lossy()});
        assert_eq!(ffmpeg_from_config(&cfg).as_deref(), Some(fake.as_path()));
        assert!(ffmpeg_from_config(&json!({"ffmpegPath":"/definitely/missing/ffmpeg"})).is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn native_cards_render_unicode_and_escape_markup() {
        let dir =
            std::env::temp_dir().join(format!("dominion-video-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        make_cards(
            &dir,
            "테스트 <제목>",
            &["한글 자막 & 실제 프레임".into()],
            "9:16",
        )
        .unwrap();
        let bytes = std::fs::read(dir.join("card-00.png")).unwrap();
        assert!(bytes.starts_with(b"\x89PNG"));
        assert!(bytes.len() > 10000);
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn multiline_storyboard_is_bounded_and_keeps_scene_order() {
        let input=json!({"brief":"첫 번째 장면

두 번째 장면

세 번째 장면"});
        assert_eq!(storyboard_scenes(&input).unwrap(),
            vec!["첫 번째 장면","두 번째 장면","세 번째 장면"]);
        assert_eq!(storyboard_scenes(&json!({"brief":"단일 장면"})).unwrap().len(),1);
        assert!(storyboard_scenes(&json!({"scenes":[1,"scene"]})).is_err());
        assert!(storyboard_scenes(&json!({"scenes":["a","b","c","d","e","f","g","h","i"]})).is_err());
    }

    #[test]
    fn late_remote_cancel_receipts_cannot_be_downgraded_by_owner_stop_races(){
        let acknowledged=json!({"status":"requested","promptId":"known"});
        let pending=json!({"status":"unconfirmed","reason":"awaiting id"});
        assert_eq!(preferable_cancel_receipt(&acknowledged,&pending),acknowledged);
        assert_eq!(preferable_cancel_receipt(&pending,&acknowledged),acknowledged);
        assert_eq!(preferable_cancel_receipt(&pending,&json!({"status":"not_active"})),pending);
    }
    #[test]
    fn installing_a_new_final_mp4_replaces_existing_output_on_all_platforms() {
        let root=std::env::temp_dir().join(format!("dominion-mp4-swap-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let target=root.join("output.mp4");
        let staged=root.join("output.part.mp4");
        std::fs::write(&target,vec![1u8;1024]).unwrap();
        std::fs::write(&staged,vec![2u8;2048]).unwrap();
        install_video_output(&staged,&target).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(),vec![2u8;2048]);
        assert!(!staged.exists());
        assert!(!root.join("output.previous.mp4").exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn resuming_uses_only_complete_matching_scene_artifacts() {
        let root=std::env::temp_dir().join(format!("dominion-resume-{}",uuid::Uuid::new_v4()));
        let scene=root.join("scene-00");
        std::fs::create_dir_all(&scene).unwrap();
        let clip=scene.join("output.mp4");
        std::fs::write(&clip,vec![42u8;4096]).unwrap();
        let fp="frozen-scene-fingerprint";
        let proof=json!({"fingerprint":fp,"sha256":file_sha256(&clip).unwrap(),
            "sceneIndex":0,"engine":"comfyui"});
        std::fs::write(scene.join("manifest.json"),proof.to_string()).unwrap();
        assert!(verified_scene_clip(&root,0,fp));
        assert!(!verified_scene_clip(&root,0,"new-input"));
        assert!(!verified_scene_clip(&root,1,fp));
        std::fs::write(&clip,vec![13u8;4096]).unwrap();
        assert!(!verified_scene_clip(&root,0,fp),"tampered bytes cannot be reused");
        std::fs::remove_file(scene.join("manifest.json")).unwrap();
        assert!(!verified_scene_clip(&root,0,fp),"uncommitted clips cannot be reused");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn structured_storyboard_produces_visuals_and_spoken_lines_without_extra_llm_calls(){
        let sample=r#"{"scenes":[{"visual":"woman in blue jacket walking","narration":"도시의 아침이 밝습니다."},{"visual":"same woman reaches the station","narration":"그녀의 하루가 시작됩니다."}]}"#;
        let (visuals,narration)=parse_video_plan(sample,4).unwrap();
        assert_eq!(visuals.len(),2);
        assert_eq!(narration.len(),2);
        let input=json!({"duration":6,"narrationLines":narration});
        let srt=captions_from_narration(&input,&narration.join(" "));
        assert!(srt.contains("00:00:00,000 --> 00:00:03,000"));
        assert!(srt.contains("00:00:03,000 --> 00:00:06,000"));
        assert!(parse_video_plan(r#"{"scenes":[{"visual":"A","narration":""},{"visual":"B","narration":"B"}]}"#,4).is_err());
        assert_eq!(parse_video_plan(r#"["A","B"]"#,4).unwrap().1.len(),0);
    }
    #[test]
    fn restored_ai_storyboard_passes_the_same_frozen_validator() {
        let scenes=json!(["Character enters a blue station",
            "Same character walks out with the same jacket"]);
        assert_eq!(parse_video_storyboard(&scenes.to_string(),4).unwrap().len(),2);
        assert!(parse_video_storyboard(&json!(["only one"]).to_string(),4).is_err());
    }

    #[test]
    fn unchanged_scene_fingerprints_are_stable_and_scene_specific() {
        let render=json!({"prompt":"same character","seed":101,"width":512});
        let workflow=json!({"node":{"class_type":"KSampler","inputs":{"seed":101}}});
        let first=scene_key(&render,&workflow,"landscape",0);
        assert_eq!(first,scene_key(&render,&workflow,"landscape",0));
        assert_ne!(first,scene_key(&render,&workflow,"landscape",1));
        assert_ne!(first,scene_key(&render,&workflow,"portrait",0));
        let mut next=render.clone();
        next["seed"]=json!(102);
        assert_ne!(first,scene_key(&next,&workflow,"landscape",0));
    }

    #[test]
    fn storyboard_duration_distribution_preserves_requested_total(){
        for (total,seconds) in [(3u64,13u64),(7,21),(8,35)] {
            let per_scene=(0..total).map(|i|
                seconds/total+u64::from(i<seconds%total)).collect::<Vec<_>>();
            assert_eq!(per_scene.iter().sum::<u64>(),seconds);
            assert!(per_scene.iter().all(|duration|*duration>=3));
        }
    }
    #[test]
    fn video_provenance_uses_streaming_sha256_for_large_files(){
        let dir=std::env::temp_dir().join(format!("dominion-video-hash-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path=dir.join("clip.mp4");
        let bytes=vec![42u8; 131_095];
        std::fs::write(&path,&bytes).unwrap();
        assert_eq!(file_sha256(&path).unwrap(),format!("{:x}",Sha256::digest(&bytes)));
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn remote_gpu_scene_budget_counts_only_real_new_renders() {
        assert!(within_new_scene_budget(0,Some(0))==false);
        assert!(within_new_scene_budget(0,Some(1)));
        assert!(!within_new_scene_budget(1,Some(1)));
        assert!(within_new_scene_budget(999,None));
    }

    #[test]
    fn crashed_model_transfer_is_reported_as_interrupted_not_forever_downloading() {
        let dir=std::env::temp_dir().join(format!(
            "dominion-download-restart-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg=crate::config::Config{
            port:0,data_dir:dir.clone(),web_dir:dir.join("web"),workspace:dir.clone()
        };
        let state=AppState::new(cfg.clone(),
            crate::db::Store::open(&cfg.db_path()).unwrap(),
            crate::vault::Vault::open(&cfg.key_path()));
        let id="video_model_download:wan21-t2v-1.3b-q4";
        state.store.put("video_model_download",&json!({
            "id":id,"model":"wan21-t2v-1.3b-q4","status":"downloading"
        }));
        let row=downloads(&state).into_iter().find(|v|v["id"]==id).unwrap();
        assert_eq!(row["status"],"interrupted");
        assert_eq!(state.store.get("video_model_download",id).unwrap()["status"],"interrupted");
        drop(state);
        let _=std::fs::remove_dir_all(dir);
    }

    #[test]
    fn auto_storyboard_uses_bounded_structured_scene_contract() {
        assert_eq!(parse_video_storyboard("[\"opening\",\"ending\"]",2).unwrap().len(),2);
        assert_eq!(parse_video_storyboard("```json\n[\"opening\",\"ending\"]\n```",2).unwrap().len(),2);
        assert!(parse_video_storyboard("[\"one\"]",2).is_err());
        assert!(parse_video_storyboard("[\"one\",\"two\",\"three\"]",2).is_err());
        assert!(parse_video_storyboard("[\"one\",17]",2).is_err());
    }

    #[test]
    fn every_video_quant_variant_has_matching_workflow_and_downloadable_manifest() {
        for (id,_,quant,kind,weight,_,_) in VIDEO_T2V_VARIANTS {
            let files=model_files(id).expect("every video variant must download");
            assert_eq!(files.len(),3);
            assert_eq!(files[0].0,weight);
            assert!(files.iter().all(|(_,url)|url.starts_with("https://huggingface.co/")));
            let graph=workflow_for(id,20).expect("every video variant must render");
            let name=graph["1"]["inputs"]["unet_name"].as_str().unwrap();
            assert_eq!(name,weight.rsplit('/').next().unwrap(),
                "workflow selected an incompatible video model {id} ({quant}, {kind})");
            if id=="wan22-ti2v-5b-fp16"{
                assert_eq!(graph["6"]["class_type"],"Wan22ImageToVideoLatent");
                assert_eq!(files[2].0,"vae/wan2.2_vae.safetensors");
            }
        }
        assert!(model_files("llama-3").is_none());
        assert!(workflow_for("llama-3",20).is_none());
    }

    #[test]
    fn multi_file_model_progress_never_claims_completion_early() {
        assert_eq!(model_download_percent(0,3,0,100),0);
        assert_eq!(model_download_percent(0,3,50,100),16);
        assert_eq!(model_download_percent(1,3,0,0),33);
        assert_eq!(model_download_percent(1,3,99,100),66);
        assert_eq!(model_download_percent(2,3,99,100),99);
        assert_eq!(model_download_percent(3,3,0,0),99);
    }

    #[test]
    fn comfy_receipts_support_legacy_and_current_job_statuses() {
        assert_eq!(comfy_result_status(&json!({"status":{"status_str":"success","completed":true}})),"completed");
        assert_eq!(comfy_result_status(&json!({"status":"completed","outputs":{}})),"completed");
        assert_eq!(comfy_result_status(&json!({"status":"in_progress"})),"running");
        assert_eq!(comfy_result_status(&json!({"status":{"status_str":"error"}})),"failed");
        assert_eq!(comfy_result_status(&json!({"status":"cancelled"})),"cancelled");
    }

    #[test]
    fn public_video_receipt_shows_storyboard_before_render_without_false_playback() {
        let queued=json!({"id":"abc","kind":"video","status":"queued",
            "input":{"engine":"comfyui","title":"Project","brief":"Plan",
                "scenes":["intro","outro"],"render":{"fps":16,"seed":42,
                    "width":480,"height":832,"frames":96}}});
        let result=public_job(&queued);
        assert_eq!(result["storyboard"],json!(["intro","outro"]));
        assert_eq!(result["renderParams"]["seed"],42);
        assert_eq!(result["renderParams"]["fps"],16);
        assert_eq!(result["sceneProgress"]["index"],Value::Null);
        assert_eq!(result["outputUrl"],Value::Null);
    }

    #[test]
    fn workload_is_honest_about_units_and_does_not_invent_prices() {
        let input=json!({"render":{"width":640,"height":360,"fps":16,"frames":240}});
        let workload=render_workload(&input);
        assert_eq!(workload["frames"],240);
        assert_eq!(workload["pixelFrames"],55_296_000u64);
        assert_eq!(workload["megapixelFrames"],55);
        assert!(workload["estimatedMoneyCost"].is_null());
    }

    #[tokio::test]
    async fn remote_cancel_requests_only_its_own_prompt_id() {
        let target_id = uuid::Uuid::new_v4().to_string();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let server_url = format!("http://{}", listener.local_addr().unwrap());
        let router = axum::Router::new().route("/api/jobs/:id/cancel",
            axum::routing::post(|axum::extract::Path(id): axum::extract::Path<String>| async move {
                axum::Json(json!({"cancelled": uuid::Uuid::parse_str(&id).is_ok()}))
            }));
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        assert!(cancel_comfy_prompt(&server_url, &target_id).await.unwrap());
        server.abort();
    }

    #[tokio::test]
    async fn remote_cancel_rejects_invalid_target_before_network_io() {
        let prompt = uuid::Uuid::new_v4().to_string();
        assert!(cancel_comfy_prompt("file:///tmp/comfy", &prompt).await.is_err());
        assert!(cancel_comfy_prompt("http://localhost:8188", "../interrupt").await.is_err());
        assert!(cancel_comfy_prompt("http://user:password@localhost:8188", &prompt).await.is_err());
        assert!(cancel_comfy_prompt("http://localhost:8188/?bad=1", &prompt).await.is_err());
    }

    #[test]
    fn template_substitution_never_changes_json_structure() {
        let mut value = json!({"1":{"inputs":{"text":"{{prompt}}","seconds":"{{duration}}"}}});
        substitute(&mut value, &json!({"prompt":"\"},evil:{","duration":15}));
        assert_eq!(value["1"]["inputs"]["text"], "\"},evil:{");
        assert_eq!(value["1"]["inputs"]["seconds"], 15);
        assert_eq!(value.as_object().unwrap().len(), 1);
    }
}
