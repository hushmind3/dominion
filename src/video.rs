use crate::provider::ModelClient;
use base64::Engine as _;
fn now_ms()->u64 { chrono::Utc::now().timestamp_millis().max(0) as u64 }
use serde::{Deserialize,Serialize};
use serde_json::{json,Value};
use std::{collections::{HashMap,HashSet},path::{Path,PathBuf},process::Stdio,sync::Arc,time::Duration};
use tokio::{fs,io::AsyncWriteExt,process::Command,sync::Mutex};
use uuid::Uuid;
#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct Scene { pub heading:String, pub narration:String, pub seconds:u32 }
#[derive(Clone,Debug,Serialize,Deserialize,PartialEq,Eq)]
#[serde(rename_all="snake_case")]
pub enum VideoStatus { Draft, Rendering, Completed, Failed }
#[derive(Clone,Debug,Serialize,Deserialize,PartialEq,Eq,Default)]
#[serde(rename_all="snake_case")]
pub enum VideoBackend { #[default] Local, Veo }
#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct Video {
    pub id:Uuid, pub prompt:String, pub scenes:Vec<Scene>, pub status:VideoStatus,
    pub cost_krw:f64, pub render_seconds:u64, pub error:Option<String>,
    pub output:Option<String>, pub created_at_ms:u64, pub updated_at_ms:u64,
    #[serde(default)] pub backend:VideoBackend,
    #[serde(default)] pub budget_krw:f64,
    #[serde(default)] pub remote_operations:HashMap<usize,String>,
    #[serde(default)] pub images:HashMap<usize,String>,
}
#[derive(Deserialize)]
pub struct NewVideo { pub prompt:String, pub scenes:Option<Vec<Scene>>, pub backend:Option<VideoBackend>,
    pub budget_krw:Option<f64> }
#[derive(Deserialize)]
pub struct ChangeScene { pub heading:String, pub narration:String, pub seconds:u32 }
pub struct Videos {
    root:PathBuf,
    state:Mutex<HashMap<Uuid,Video>>,
    active:Mutex<HashSet<Uuid>>,
}
impl Videos {
    pub async fn open(root:PathBuf)->Result<Self,String>{
        fs::create_dir_all(&root).await.map_err(|e|e.to_string())?;
        let mut state=HashMap::new();
        let mut dir=fs::read_dir(&root).await.map_err(|e|e.to_string())?;
        while let Some(entry)=dir.next_entry().await.map_err(|e|e.to_string())?{
            if entry.path().extension().and_then(|s|s.to_str())!=Some("json"){continue;}
            let mut video:Video=serde_json::from_slice(&fs::read(entry.path()).await.map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
            if video.status==VideoStatus::Rendering {
                video.status=VideoStatus::Failed;
                video.error=Some("Render interrupted; rerun to resume from saved clips".into());
            }
            state.insert(video.id,video);
        }
        Ok(Self{root,state:Mutex::new(state),active:Mutex::new(HashSet::new())})
    }
    async fn put(&self,video:Video)->Result<Video,String>{
        let mut guard=self.state.lock().await;
        let tmp=self.root.join(format!("{}.tmp",video.id));
        let dst=self.root.join(format!("{}.json",video.id));
        fs::write(&tmp,serde_json::to_vec(&video).map_err(|e|e.to_string())?).await.map_err(|e|e.to_string())?;
        fs::rename(tmp,dst).await.map_err(|e|e.to_string())?;
        guard.insert(video.id,video.clone());Ok(video)
    }
    pub async fn get(&self,id:Uuid)->Option<Video>{self.state.lock().await.get(&id).cloned()}
    pub async fn list(&self)->Vec<Video>{
        let mut all:Vec<_>=self.state.lock().await.values().cloned().collect();
        all.sort_by_key(|v|std::cmp::Reverse(v.created_at_ms));all
    }
    pub async fn create(&self,model:&ModelClient,input:NewVideo)->Result<Video,String>{
        let prompt=input.prompt.trim();
        if prompt.is_empty()||prompt.len()>10_000{return Err("video prompt must be 1..10000 characters".into());}
        let backend=input.backend.unwrap_or_default();
        if backend==VideoBackend::Veo { let _=crate::veo::Veo::from_env()?; }
        let budget=input.budget_krw.unwrap_or(if backend==VideoBackend::Veo {0.0}else{5000.0});
        if !budget.is_finite()||budget<0.0||budget>1_000_000.0 ||
            (backend==VideoBackend::Veo && budget==0.0) {
            return Err("Explicit positive KRW budget required for paid video generation".into());
        }
        let (scenes,cost)=match input.scenes {
            Some(items)=>(items,0.0),
            None=>{
                let requests=vec![
                    json!({"role":"system","content":format!("You write production-ready short-form video storyboards. Output only JSON: {{\"scenes\":[{{\"heading\":\"...\",\"narration\":\"...\",\"seconds\":4}}]}}. Provide 3..6 coherent scenes, concise narration, a satisfying ending, in the requested language. {}",
                        if backend==VideoBackend::Veo {"Every scene MUST have duration 4, 6 or 8 seconds."}else{"Each scene should be 4..10 seconds."})}),
                    json!({"role":"user","content":prompt}),
                ];
                let answer=model.complete(&requests,&[],true).await?;
                let raw=answer.text.trim();
                let text=raw.strip_prefix("```json").or_else(||raw.strip_prefix("```")).unwrap_or(raw).trim_end_matches("```").trim();
                let doc:Value=serde_json::from_str(text).map_err(|e|format!("storyboard JSON invalid: {e}"))?;
                let parsed=serde_json::from_value(doc.get("scenes").cloned().ok_or("storyboard has no scenes")?).map_err(|e|format!("storyboard scene invalid: {e}"))?;
                (parsed,answer.cost_krw)
            },
        };
        validate_scenes(&scenes)?;
        if backend==VideoBackend::Veo && scenes.iter().any(|scene|!matches!(scene.seconds,4|6|8)) {
            return Err("Veo scenes must be 4, 6 or 8 seconds".into());
        }
        let now=now_ms();
        let video=Video{id:Uuid::new_v4(),prompt:prompt.into(),scenes,status:VideoStatus::Draft,cost_krw:cost,render_seconds:0,error:None,output:None,created_at_ms:now,updated_at_ms:now,backend,budget_krw:budget,remote_operations:HashMap::new(),images:HashMap::new()};
        self.put(video).await
    }
    /// A user's PNG/JPEG image becomes a private, deterministic local asset.
    /// This never downloads from untrusted URLs and never runs a model.
    pub async fn set_scene_image(&self,id:Uuid,index:usize,data_url:&str)->Result<Video,String>{
        if self.active.lock().await.contains(&id){return Err("render in progress".into());}
        let mut video=self.get(id).await.ok_or("video not found")?;
        if video.backend!=VideoBackend::Local{return Err("Images are for the local video renderer".into());}
        if index>=video.scenes.len(){return Err("scene out of range".into());}
        if data_url.len()>7_100_000{return Err("Image exceeds 5 MiB".into());}
        let (kind,payload)=data_url.split_once(',').ok_or("Image data URI required")?;
        let extension=match kind{
            "data:image/png;base64"=>"png",
            "data:image/jpeg;base64"=>"jpg",
            _=>return Err("Only PNG and JPEG images are accepted".into()),
        };
        let bytes=base64::engine::general_purpose::STANDARD.decode(payload)
            .map_err(|_|"Image base64 invalid")?;
        if bytes.is_empty() || bytes.len()>5*1024*1024 {
            return Err("Image must contain 1..5 MiB".into());
        }
        let format_ok=if extension=="png"{
            bytes.starts_with(&[137,80,78,71,13,10,26,10])
        }else {bytes.starts_with(&[255,216,255])};
        if !format_ok{return Err("Image signature does not match its MIME type".into());}
        let dir=self.root.join(id.to_string());
        fs::create_dir_all(&dir).await.map_err(|e|e.to_string())?;
        let relative=format!("scene-{index}.{extension}");
        let tmp=dir.join(format!("scene-{index}.upload"));
        fs::write(&tmp,&bytes).await.map_err(|e|e.to_string())?;
        fs::rename(&tmp,dir.join(&relative)).await.map_err(|e|e.to_string())?;
        let prior=video.images.insert(index,relative.clone());
        if let Some(previous)=prior {
            if previous!=relative {let _=fs::remove_file(dir.join(previous)).await;}
        }
        let _=fs::remove_file(dir.join(format!("scene-{index}.mp4"))).await;
        video.status=VideoStatus::Draft;video.output=None;video.error=None;
        video.updated_at_ms=now_ms();
        self.put(video).await
    }

    pub async fn change_scene(&self,id:Uuid,index:usize,scene:ChangeScene)->Result<Video,String>{
        if self.active.lock().await.contains(&id){return Err("render in progress".into());}
        let mut video=self.get(id).await.ok_or("video not found")?;
        if index>=video.scenes.len(){return Err("scene out of range".into());}
        let replacement=Scene{heading:scene.heading,narration:scene.narration,seconds:scene.seconds};
        validate_scenes(std::slice::from_ref(&replacement))?;
        if video.backend==VideoBackend::Veo && !matches!(replacement.seconds,4|6|8) {
            return Err("Veo scene must be 4, 6 or 8 seconds".into());
        }
        video.remote_operations.remove(&index);
        video.scenes[index]=replacement;
        video.status=VideoStatus::Draft;video.error=None;video.output=None;video.updated_at_ms=now_ms();
        let clip=self.root.join(id.to_string()).join(format!("scene-{index}.mp4"));
        let _=fs::remove_file(clip).await; // preserve unchanged scenes for partial regeneration
        self.put(video).await
    }
    pub async fn start(self:&Arc<Self>,id:Uuid)->Result<Video,String>{
        {let mut lock=self.active.lock().await;if !lock.insert(id){return Err("render already active".into());}}
        let mut video=match self.get(id).await {
            Some(v)=>v,
            None=>{self.active.lock().await.remove(&id);return Err("video not found".into());}
        };
        video.status=VideoStatus::Rendering;
        video.error=None;video.output=None;video.updated_at_ms=now_ms();
        if let Err(e)=self.put(video.clone()).await {self.active.lock().await.remove(&id);return Err(e);}
        let videos=Arc::clone(self);
        tokio::spawn(async move{
            let begin=std::time::Instant::now();
            let result=videos.render(id).await;
            if let Some(mut video)=videos.get(id).await{
                video.render_seconds=video.render_seconds.saturating_add(begin.elapsed().as_secs());
                video.updated_at_ms=now_ms();
                match result {
                    Ok(file)=>{video.status=VideoStatus::Completed;video.output=Some(file);video.error=None;}
                    Err(error)=>{video.status=VideoStatus::Failed;video.output=None;video.error=Some(error);}
                }
                if let Err(error)=videos.put(video).await{eprintln!("video persist failed: {error}");}
            }
            videos.active.lock().await.remove(&id);
        });
        Ok(video)
    }
    async fn render(&self,id:Uuid)->Result<String,String>{
        let video=self.get(id).await.ok_or("video not found")?;
        let directory=self.root.join(id.to_string());
        fs::create_dir_all(&directory).await.map_err(|e|e.to_string())?;
        if video.backend==VideoBackend::Veo {
            return self.render_veo(id,&video,&directory).await;
        }
        for (index,scene) in video.scenes.iter().enumerate(){
            let clip=directory.join(format!("scene-{index}.mp4"));
            if fs::metadata(&clip).await.is_ok(){continue;}
            let textfile=directory.join(format!("scene-{index}.txt"));
            let text=scene_caption(scene);
            fs::write(&textfile,&text).await.map_err(|e|e.to_string())?;
            let voice=directory.join(format!("scene-{index}.wav"));
            let speech=make_speech(&directory,&scene.narration,&voice).await;
            let color=["0x16263f","0x202a48","0x27384b","0x172c3c","0x2d2948","0x24414a"][index%6];
            let filter=format!("drawtext=textfile=scene-{index}.txt:fontcolor=white:fontsize=47:line_spacing=18:x=(w-text_w)/2:y=(h-text_h)/2:box=1:boxcolor=black@0.50:boxborderw=26");
            let mut command=Command::new("ffmpeg");
            command.current_dir(&directory).arg("-y").arg("-hide_banner").arg("-loglevel").arg("error");
            let image=video.images.get(&index).filter(|name|
                *name==&format!("scene-{index}.png") || *name==&format!("scene-{index}.jpg"))
                .map(|name|directory.join(name)).filter(|path|path.is_file());
            if let Some(image)=&image {
                command.arg("-loop").arg("1").arg("-framerate").arg("24")
                    .arg("-i").arg(image);
            }else{
                command.arg("-f").arg("lavfi").arg("-i")
                    .arg(format!("color=c={color}:s=1280x720:r=24:d={}",scene.seconds));
            }
            if speech {
                command.arg("-i").arg(&voice).arg("-af").arg("apad");
            }else{
                command.arg("-f").arg("lavfi").arg("-i").arg("anullsrc=channel_layout=stereo:sample_rate=44100");
            }
            let visual=if image.is_some() {
                format!("scale=1380:776:force_original_aspect_ratio=increase,crop=1380:776,zoompan=z='min(zoom+0.0006,1.12)':d=1:s=1280x720:fps=24,eq=contrast=1.05,{filter}")
            }else{filter};
            command.arg("-vf").arg(&visual).arg("-c:v").arg("libx264").arg("-preset").arg("veryfast")
                .arg("-crf").arg("24").arg("-pix_fmt").arg("yuv420p").arg("-c:a").arg("aac")
                .arg("-b:a").arg("112k").arg("-t").arg(scene.seconds.to_string())
                .arg("-shortest").arg(&clip);
            invoke(command,Duration::from_secs(120)).await?;
        }
        self.merge_clips(id,video.scenes.len(),&directory).await
    }
    async fn render_veo(&self,id:Uuid,video:&Video,directory:&Path)->Result<String,String>{
        let veo=crate::veo::Veo::from_env()?;
        for (index,scene) in video.scenes.iter().enumerate(){
            let clip=directory.join(format!("scene-{index}.mp4"));
            if fs::metadata(&clip).await.is_ok(){continue;}
            let mut latest=self.get(id).await.ok_or("video not found")?;
            let operation=if let Some(id)=latest.remote_operations.get(&index){id.clone()}else{
                let prompt=format!("{}\n\nVisual direction: {}\n\nDialogue and narration: {}",
                    video.prompt,scene.heading,scene.narration);
                let expected=veo.estimate_krw(scene.seconds);
                if !expected.is_finite() || expected<=0.0 || latest.cost_krw+expected>video.budget_krw {
                    return Err(format!("Video budget exceeded: spent {:.2} KRW, estimated next {:.2} KRW, limit {:.2} KRW",latest.cost_krw,expected,video.budget_krw));
                }
                let operation=veo.start(&prompt,scene.seconds).await?;
                latest.remote_operations.insert(index,operation.clone());
                latest.cost_krw+=veo.estimate_krw(scene.seconds);
                latest.updated_at_ms=now_ms();
                self.put(latest).await?;
                operation
            };
            let raw=directory.join(format!("scene-{index}.original.mp4"));
            if fs::metadata(&raw).await.is_err(){
                let url=veo.wait(&operation).await?;
                veo.download(&url,&raw).await?;
            }
            let mut command=Command::new("ffmpeg");
            command.current_dir(directory).arg("-y").arg("-hide_banner").arg("-loglevel").arg("error")
                .arg("-i").arg(&raw).arg("-map").arg("0:v:0").arg("-map").arg("0:a:0?")
                .arg("-vf").arg("scale=1280:720:force_original_aspect_ratio=decrease,pad=1280:720:(ow-iw)/2:(oh-ih)/2,fps=24")
                .arg("-c:v").arg("libx264").arg("-preset").arg("veryfast")
                .arg("-crf").arg("24").arg("-pix_fmt").arg("yuv420p")
                .arg("-c:a").arg("aac").arg("-ar").arg("44100").arg("-ac").arg("2")
                .arg("-t").arg(scene.seconds.to_string()).arg(&clip);
            invoke(command,Duration::from_secs(150)).await?;
        }
        self.merge_clips(id,video.scenes.len(),directory).await
    }
    async fn merge_clips(&self,id:Uuid,count:usize,directory:&Path)->Result<String,String>{
        let manifest=(0..count).map(|i|format!("file 'scene-{i}.mp4'")).collect::<Vec<_>>().join("\n");
        fs::write(directory.join("concat.txt"),format!("{manifest}\n")).await.map_err(|e|e.to_string())?;
        let tmp=directory.join("rendering.mp4");
        let final_name=directory.join("finished.mp4");
        let mut merge=Command::new("ffmpeg");
        merge.current_dir(directory).arg("-y").arg("-hide_banner").arg("-loglevel").arg("error")
            .arg("-f").arg("concat").arg("-safe").arg("0").arg("-i").arg("concat.txt")
            .arg("-c").arg("copy").arg(&tmp);
        invoke(merge,Duration::from_secs(120)).await?;
        // Portable atomic-ish replacement on Windows as well as POSIX.
        // Keep the last successful artifact recoverable if a rename fails.
        let old=directory.join("previous.mp4");
        let had_previous=fs::metadata(&final_name).await.is_ok();
        if had_previous {
            let _=fs::remove_file(&old).await;
            fs::rename(&final_name,&old).await.map_err(|e|e.to_string())?;
        }
        if let Err(error)=fs::rename(&tmp,&final_name).await {
            if had_previous {let _=fs::rename(&old,&final_name).await;}
            return Err(error.to_string());
        }
        if had_previous {let _=fs::remove_file(old).await;}
        Ok(format!("{id}/finished.mp4"))
    }
    pub async fn output_path(&self,id:Uuid)->Result<PathBuf,String>{
        let video=self.get(id).await.ok_or("video not found")?;
        if video.status!=VideoStatus::Completed{return Err("video not completed".into());}
        let path=video.output.ok_or("no rendered video")?;
        Ok(self.root.join(path))
    }
}
/// Caption text must fit inside the fixed video frame. Full narration stays
/// in the audio track and metadata; onscreen copy is a legible synopsis.
fn scene_caption(scene:&Scene)->String {
    let heading=scene.heading.trim().chars().take(34).collect::<String>();
    let mut lines=vec![];
    let mut line=String::new();
    let mut width=0usize;
    let mut clipped=false;
    for character in scene.narration.trim().chars() {
        if character=='\n' || character=='\r' {
            if !line.is_empty(){lines.push(std::mem::take(&mut line));}
            width=0;
            if lines.len()>=2 {clipped=true;break;}
            continue;
        }
        let cells=if character.is_ascii(){1}else{2};
        if width+cells>38 {
            lines.push(std::mem::take(&mut line));
            width=0;
            if lines.len()>=2 {clipped=true;break;}
        }
        line.push(character);
        width+=cells;
    }
    if lines.len()<2 && !line.is_empty(){lines.push(line);}
    let mut subtitle=lines.join("\n");
    if clipped {subtitle.push('…');}
    if subtitle.is_empty(){heading}else{format!("{heading}\n\n{subtitle}")}
}
fn validate_scenes(scenes:&[Scene])->Result<(),String>{
    if scenes.is_empty()||scenes.len()>12{return Err("video needs 1..12 scenes".into());}
    let mut total=0u32;
    for scene in scenes {
        if scene.heading.trim().is_empty()||scene.heading.len()>180||scene.narration.len()>1000||!(2..=30).contains(&scene.seconds) {
            return Err("invalid scene heading, narration or duration".into());
        }
        total=total.saturating_add(scene.seconds);
    }
    if total>180 {return Err("video duration exceeds 180 seconds".into());}
    Ok(())
}
/// Prefer Piper's offline neural speech when a local voice is configured.
/// It uses no per-character cloud API and runs on typical CPU-only machines.
/// The explicit eSpeak fallback keeps rendering available without a large model.
async fn make_speech(dir:&Path,text:&str,voice:&Path)->bool{
    if std::env::var("DOMINION_DISABLE_TTS").ok().as_deref()==Some("true")
        || text.trim().is_empty(){return false;}
    if let Ok(model)=std::env::var("DOMINION_PIPER_MODEL") {
        let model=Path::new(&model);
        if model.is_file() {
            let binary=std::env::var("DOMINION_PIPER_BIN")
                .unwrap_or_else(|_|"piper".into());
            let mut command=Command::new(binary);
            command.current_dir(dir).arg("--model").arg(model)
                .arg("--output_file").arg(voice).stdin(Stdio::piped())
                .stdout(Stdio::null()).stderr(Stdio::piped()).kill_on_drop(true);
            if let Ok(mut child)=command.spawn() {
                if let Some(mut input)=child.stdin.take() {
                    let completed=tokio::time::timeout(Duration::from_secs(60),async{
                        input.write_all(text.as_bytes()).await?;
                        input.write_all(b"\n").await?;
                        drop(input);
                        child.wait_with_output().await
                    }).await;
                    if matches!(completed,Ok(Ok(output)) if output.status.success())
                        && fs::metadata(voice).await.is_ok_and(|meta|meta.len()>44) {
                        return true;
                    }
                }
            }
        }
    }
    let mut command=Command::new("espeak");
    command.current_dir(dir).arg("-s").arg("155").arg("-w").arg(voice).arg(text);
    invoke(command,Duration::from_secs(20)).await.is_ok()
        && fs::metadata(voice).await.is_ok_and(|meta|meta.len()>44)
}
async fn invoke(mut command:Command,deadline:Duration)->Result<(),String>{
    command.stdout(Stdio::null()).stderr(Stdio::piped()).kill_on_drop(true);
    let output=tokio::time::timeout(deadline,command.output()).await.map_err(|_|"render subprocess timeout".to_string())?.map_err(|e|e.to_string())?;
    if output.status.success(){Ok(())}else{
        Err(format!("render subprocess failed: {}",String::from_utf8_lossy(&output.stderr).chars().take(1600).collect::<String>()))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn readable_caption_keeps_voice_text_outside_the_visual_layout(){
        let scene=Scene{heading:"제품 사용 가이드".into(),
            narration:"아주 긴 설명을 여러 화면에 걸쳐서 읽기 어렵도록 반복하는 것보다 화면에는 핵심 문장만 보여주어야 합니다. ".repeat(20),
            seconds:6};
        let caption=scene_caption(&scene);
        assert!(caption.lines().count()<=4);
        assert!(caption.len()<200);
        assert!(caption.contains("제품 사용 가이드"));
    }
    #[test]
    fn caps_video_duration(){
        let scenes=(0..7).map(|_|Scene{heading:"Scene".into(),narration:"Voice".into(),seconds:30}).collect::<Vec<_>>();
        assert!(validate_scenes(&scenes).is_err());
    }
    #[tokio::test]
    async fn persists_scene_revisions(){
        let dir=std::env::temp_dir().join(format!("dominion-video-test-{}",Uuid::new_v4()));
        let store=Videos::open(dir.clone()).await.unwrap();
        let video=Video{id:Uuid::new_v4(),prompt:"demo".into(),scenes:vec![Scene{heading:"old".into(),narration:"old".into(),seconds:4}],status:VideoStatus::Draft,cost_krw:0.0,render_seconds:0,error:None,output:None,created_at_ms:now_ms(),updated_at_ms:now_ms(),backend:VideoBackend::Local,budget_krw:5000.0,remote_operations:HashMap::new(),images:HashMap::new()};
        let id=video.id;store.put(video).await.unwrap();
        store.change_scene(id,0,ChangeScene{heading:"new".into(),narration:"text".into(),seconds:4}).await.unwrap();
        assert_eq!(store.get(id).await.unwrap().scenes[0].heading,"new");
        fs::remove_dir_all(dir).await.unwrap();
    }
}

#[cfg(test)]
mod production_tests {
    use super::*;
    #[tokio::test]
    async fn renders_real_mp4_from_manual_scenes() {
        if Command::new("ffmpeg").arg("-version").output().await.is_err() { return; }
        let dir=std::env::temp_dir().join(format!("dominion-video-production-{}",Uuid::new_v4()));
        let model=ModelClient::new(crate::provider::ModelConfig::basic(
            "http://127.0.0.1:11434/v1/chat/completions".into(),"local".into(),None
        ),None).unwrap();
        let videos=Arc::new(Videos::open(dir.clone()).await.unwrap());
        let video=videos.create(&model,NewVideo{
            prompt:"A short smoke test".into(),
            scenes:Some(vec![Scene{heading:"Runtime".into(),narration:"The local renderer produces an artifact.".into(),seconds:2}]),backend:None,budget_krw:None
        }).await.unwrap();
        let id=video.id;
        videos.start(id).await.unwrap();
        let mut result=None;
        for _ in 0..150 {
            tokio::time::sleep(Duration::from_millis(200)).await;
            let snapshot=videos.get(id).await.unwrap();
            if snapshot.status!=VideoStatus::Rendering {result=Some(snapshot);break;}
        }
        let result=result.expect("ffmpeg render timed out");
        assert_eq!(result.status,VideoStatus::Completed,"render error: {:?}",result.error);
        let file=videos.output_path(id).await.unwrap();
        assert!(fs::metadata(&file).await.unwrap().len()>1000);
        fs::remove_dir_all(dir).await.unwrap();
    }
}
