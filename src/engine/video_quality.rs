//! Deterministic media-output gate. This is container/stream validation,
//! not an aesthetic judgement or a claim of temporal scene consistency.
use serde_json::{json,Value};
use std::{io::Read,path::Path};

pub async fn inspect(file:&Path,encoder:Option<&Path>)->Result<Value,String>{
    let bytes=std::fs::metadata(file).map_err(|e|format!("영상 산출물이 없습니다: {e}"))?.len();
    if bytes<100 {return Err("영상 파일이 비어 있습니다.".into());}
    let mut header=[0u8;12];
    std::fs::File::open(file).map_err(|e|e.to_string())?
        .read_exact(&mut header).map_err(|e|e.to_string())?;
    if &header[4..8]!=b"ftyp" {
        return Err("MP4 컨테이너 헤더 검증에 실패했습니다.".into());
    }
    let Some(encoder)=encoder else{
        return Ok(json!({"status":"container_only","bytes":bytes,
            "warning":"FFprobe가 없어 실제 비디오 스트림 검증이 생략되었습니다."}));
    };
    let binary=encoder.with_file_name(if cfg!(windows){"ffprobe.exe"}else{"ffprobe"});
    if !binary.is_file(){
        return Ok(json!({"status":"container_only","bytes":bytes,
            "warning":"FFprobe가 없어 실제 비디오 스트림 검증이 생략되었습니다."}));
    }
    let output=tokio::time::timeout(std::time::Duration::from_secs(15),
        tokio::process::Command::new(&binary).args([
            "-v","error","-show_entries","format=duration:stream=codec_type,width,height",
            "-of","json"
        ]).arg(file).output()).await
        .map_err(|_|"영상 검사 도구가 제한시간을 초과했습니다.")?
        .map_err(|e|format!("영상 검사 실행 실패: {e}"))?;
    if !output.status.success(){
        return Err("MP4 비디오 스트림 검증에 실패했습니다.".into());
    }
    let probe:Value=serde_json::from_slice(&output.stdout)
        .map_err(|_|"FFprobe 출력 해석 오류")?;
    let seconds=probe["format"]["duration"].as_str()
        .and_then(|v|v.parse::<f64>().ok()).unwrap_or(0.0);
    if !seconds.is_finite()||seconds<0.25||seconds>3600.0{
        return Err("영상 재생 길이가 올바르지 않습니다.".into());
    }
    let stream=probe["streams"].as_array().and_then(|rows|rows.iter()
        .find(|row|row["codec_type"]=="video"
            && row["width"].as_u64().unwrap_or(0)>0
            && row["height"].as_u64().unwrap_or(0)>0))
        .ok_or("영상에 실제 비디오 스트림이 없습니다.")?;
    Ok(json!({"status":"passed","bytes":bytes,"durationSeconds":seconds,
        "width":stream["width"],"height":stream["height"],
        "audioPresent":probe["streams"].as_array().is_some_and(|rows|
            rows.iter().any(|s|s["codec_type"]=="audio"))}))
}

#[cfg(test)]
mod tests{
    use super::*;
    #[tokio::test]
    async fn rejects_non_mp4_and_truncated_outputs(){
        let file=std::env::temp_dir().join(format!("dominion-video-q-{}.mp4",uuid::Uuid::new_v4()));
        std::fs::write(&file,vec![0u8;512]).unwrap();
        assert!(inspect(&file,None).await.unwrap_err().contains("MP4"));
        std::fs::write(&file,b"tiny").unwrap();
        assert!(inspect(&file,None).await.unwrap_err().contains("비어"));
        std::fs::remove_file(file).unwrap();
    }
}
