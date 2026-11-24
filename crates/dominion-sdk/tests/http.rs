#![cfg(feature="http")]
use dominion_sdk::{Client,StartRun,SourceRef,RunStatus};
use serde_json::{json,Value};
use tokio::{net::TcpListener,io::{AsyncReadExt,AsyncWriteExt}};

#[tokio::test]
async fn external_http_consumer_sends_exact_start_contract(){
    let listener=TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url=format!("http://{}",listener.local_addr().unwrap());
    let server=tokio::spawn(async move{
        let (mut socket,_)=listener.accept().await.unwrap();
        let mut request=Vec::new();
        let mut chunk=[0u8;4096];
        let (header_end,length)=loop{
            let n=socket.read(&mut chunk).await.unwrap();
            assert!(n>0);
            request.extend_from_slice(&chunk[..n]);
            if let Some(index)=request.windows(4).position(|w|w==b"\r\n\r\n"){
                let end=index+4;
                let headers=String::from_utf8_lossy(&request[..end]).to_ascii_lowercase();
                assert!(headers.starts_with("post /api/sdk/runs http/1.1"));
                assert!(headers.contains("x-dominion-token: local-test-token"));
                let length=headers.lines().find_map(|line|line.strip_prefix("content-length:"))
                    .unwrap().trim().parse::<usize>().unwrap();
                break (end,length);
            }
        };
        while request.len()<header_end+length{
            let n=socket.read(&mut chunk).await.unwrap();
            assert!(n>0);request.extend_from_slice(&chunk[..n]);
        }
        let body:Value=serde_json::from_slice(&request[header_end..header_end+length]).unwrap();
        assert_eq!(body["idempotencyKey"],"request-1");
        assert_eq!(body["source"],json!({"kind":"issue","id":"42"}));
        assert_eq!(body["providerId"],"provider");
        assert_eq!(body["model"],"model");
        let reply=json!({"id":"r","instruction":"goal","status":"queued","actions":{"cancel":true}}).to_string();
        socket.write_all(format!("HTTP/1.1 202 Accepted\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",reply.len(),reply).as_bytes()).await.unwrap();
    });
    let client=Client::connect(&url).unwrap().with_admin_token("local-test-token");
    let run=client.start_run(&StartRun{instruction:"goal".into(),provider_id:Some("provider".into()),model:Some("model".into()),idempotency_key:Some("request-1".into()),source:Some(SourceRef{kind:"issue".into(),id:"42".into()}),..Default::default()}).await.unwrap();
    assert_eq!(run.status,RunStatus::Queued);
    assert!(run.actions.unwrap().cancel);
    server.await.unwrap();
}
