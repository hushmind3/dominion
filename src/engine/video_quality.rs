//! Optional real-media technical validation; independent of generative
//! quality claims. A playable container is not proof of character consistency,
//! lip-sync or aesthetics. Missing ffprobe never masquerades as a pass.
use serde_json::{json,Value};
use std::{path::Path,time::Duration};

fn fps(rate:&str)->Option<f64>{
    let (n,d)=rate.split_once('/').unwrap_or((rate,"1"));
    let n=n.parse::<f64>().ok()?;
    let d=d.parse::<f64>().ok()?;
    if n>0.0 && d>0.0 {Some(n/d)} else{None}
}
fn evaluate(metadata:&Value,expected:u64,wants_audio:bool,wants_subtitles:bool)->Value{
    let streams=metadata["streams"].as_array().cloned().unwrap_or_default();
    let video=streams.iter().find(|stream|stream["codec_type"]=="video");
    let audio=streams.iter().any(|stream|stream["codec_type"]=="audio");
    let subtitles=streams.iter().any(|stream|stream["codec_type"]=="subtitle");
    let duration=metadata["format"]["duration"].as_str()
        .and_then(|s|s.parse::<f64>().ok())
        .or_else(||metadata["format"]["duration"].as_f64());
    let mut errors=Vec::<String>::new();
    let mut warnings=Vec::<String>::new();
    if video.is_none(){errors.push("MP4에 영상 스트림이 없습니다.".into());}
    if let Some(v)=video{
        if v["width"].as_u64().unwrap_or(0)==0 || v["height"].as_u64().unwrap_or(0)==0{
            errors.push("영상 해상도가 유효하지 않습니다.".into());
        }
    }
    if duration.is_none_or(|secs|!secs.is_finite() || secs<=0.1){
        errors.push("실제 재생 길이를 확인할 수 없습니다.".into());
    }else if let Some(actual)=duration {
        if expected>0{
            let tolerance=0.5f64.max(3.0/video.and_then(|v|
                v["r_frame_rate"].as_str().and_then(fps)).unwrap_or(24.0));
            if (actual-expected as f64).abs()>tolerance{
                warnings.push(format!("설정 길이 {expected}초와 출력 길이 {actual:.2}초가 다릅니다."));
            }
        }
    }
    if wants_audio && !audio {errors.push("음성 합성 완료 표시와 실제 오디오 트랙이 다릅니다.".into());}
    if wants_subtitles && !subtitles {
        errors.push("자막 생성 완료 표시와 실제 자막 트랙이 다릅니다.".into());
    }
    let status=if !errors.is_empty(){"failed"}
        else if !warnings.is_empty(){"warning"}else{"passed"};
    json!({"status":status,"videoCodec":video.map(|v|v["codec_name"].clone()),
        "width":video.and_then(|v|v["width"].as_u64()),
        "height":video.and_then(|v|v["height"].as_u64()),
        "fps":video.and_then(|v|v["r_frame_rate"].as_str().and_then(fps)),
        "durationSeconds":duration,
        "audio":audio,"subtitles":subtitles,
        "errors":errors,"warnings":warnings,
        "semanticContinuity":"unverified","lipSync":"unverified",
        "evidence":"ffprobe_on_final_mp4"})
}
pub async fn inspect(ffprobe:&Path,mp4:&Path,expected:u64,
    wants_audio:bool,wants_subtitles:bool)->Value{
    let mut command=tokio::process::Command::new(ffprobe);
    command.args(["-v","error","-show_entries",
        "format=duration:stream=codec_type,codec_name,width,height,r_frame_rate",
        "-of","json"]).arg(mp4);
    let output=match crate::engine::tools::run_command(&mut command,Duration::from_secs(20)).await{
        Ok(o) if o.status.success()=>o,
        Ok(o)=>return json!({"status":"unverified","evidence":"ffprobe_failed",
            "reason":String::from_utf8_lossy(&o.stderr).chars().take(240).collect::<String>()}),
        Err(e)=>return json!({"status":"unverified","evidence":"ffprobe_unavailable","reason":e}),
    };
    let Ok(probe)=serde_json::from_slice::<Value>(&output.stdout) else{
        return json!({"status":"unverified","evidence":"invalid_ffprobe_output"});
    };
    evaluate(&probe,expected,wants_audio,wants_subtitles)
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn media_streams_are_measured_not_inferred_from_output_filename(){
        let sample=json!({"format":{"duration":"6.00"},
            "streams":[{"codec_type":"video","codec_name":"h264",
                "width":1280,"height":720,"r_frame_rate":"24/1"},
                {"codec_type":"subtitle","codec_name":"mov_text"}]});
        let ok=evaluate(&sample,6,false,true);
        assert_eq!(ok["status"],"passed");
        assert_eq!(ok["audio"],false);
        assert_eq!(ok["semanticContinuity"],"unverified");
        assert_eq!(evaluate(&sample,6,true,true)["status"],"failed");
        assert_eq!(evaluate(&sample,6,false,false)["status"],"passed");
        let mut bad=sample.clone();bad["streams"]=json!([]);
        assert_eq!(evaluate(&bad,6,false,false)["status"],"failed");
        let warning=evaluate(&sample,10,false,true);
        assert_eq!(warning["status"],"warning");
        assert_eq!(fps("25/0"),None);
    }
}
