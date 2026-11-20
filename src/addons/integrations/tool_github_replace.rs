//! Exact one-block GitHub edit. The model never has to send the entire file.
use super::{github_file_read, github_file_update, ToolContext};
use serde_json::{json, Value};

fn change_once(content:&str,old:&str,new:&str)->Result<String,String>{
    if old.is_empty(){return Err("원본 구간은 비어 있을 수 없습니다.".into());}
    let mut positions=content.match_indices(old);
    let start=positions.next().map(|(index,_)|index)
        .ok_or("변경할 원문 구간이 없습니다. GitHub 파일을 다시 읽으세요.")?;
    if positions.next().is_some(){
        return Err("수정 구간이 여러 번 등장합니다. 더 정확한 문맥을 지정하세요.".into());
    }
    let mut updated=String::with_capacity(content.len()-old.len()+new.len());
    updated.push_str(&content[..start]);updated.push_str(new);
    updated.push_str(&content[start+old.len()..]);
    if updated==content{return Err("변경 내용이 원본과 같습니다.".into());}
    Ok(updated)
}

pub(super) async fn replace(ctx:&ToolContext<'_>,args:&Value)->Result<Value,String>{
    let expected=args["expectedSha"].as_str().ok_or("원본 파일 SHA를 지정해야 합니다.")?;
    if expected.len()!=40||!expected.bytes().all(|byte|byte.is_ascii_hexdigit()){
        return Err("github_file_read에서 받은 40자리 파일 SHA가 필요합니다.".into());
    }
    let old=args["oldText"].as_str().ok_or("기존 텍스트가 필요합니다.")?;
    let new=args["newText"].as_str().ok_or("새 텍스트가 필요합니다.")?;
    if old.len()>256*1024 || new.len()>256*1024{
        return Err("변경 구간은 256KiB 이하여야 합니다.".into());
    }
    if args["branch"].as_str().is_none(){return Err("수정할 브랜치를 지정하세요.".into());}
    let observed=github_file_read(ctx,args).await?;
    if observed["sha"].as_str()!=Some(expected){
        return Err("원격 파일이 변경되었습니다. 최신 SHA로 다시 읽어 수정하세요.".into());
    }
    let content=observed["content"].as_str().ok_or("GitHub 파일 내용이 없습니다.")?;
    let changed=change_once(content,old,new)?;
    let input=json!({
        "owner":args["owner"],"repo":args["repo"],"path":args["path"],
        "branch":args["branch"],"expectedSha":expected,
        "message":args["message"],"content":changed
    });
    let mut result=github_file_update(ctx,&input).await?;
    result["replacement"]=json!(true);
    result["oldTextBytes"]=json!(old.len());
    result["newTextBytes"]=json!(new.len());
    Ok(result)
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn exact_block_update_is_unique_and_preserves_other_bytes(){
        assert_eq!(change_once("before OLD after","OLD","NEW").unwrap(),"before NEW after");
        assert_eq!(change_once("a\nb\n","b\n","").unwrap(),"a\n");
        assert!(change_once("one one","one","new").is_err());
        assert!(change_once("first","other","new").is_err());
        assert!(change_once("first","","new").is_err());
        assert!(change_once("first","first","first").is_err());
    }
}
