use super::*;
use crate::{config::Config,db::Store,vault::Vault};
#[tokio::test]
#[ignore = "Real upstream runtime: run with DOMINION_OPENCLAW_ROOT and DOMINION_PLUGIN_NODE"]
async fn real_sdk_plugin_install_service_hook_tool_disable_and_remove(){
    assert!(std::env::var_os("DOMINION_OPENCLAW_ROOT").is_some(),"Provide the actual upstream runtime");
    let dir=std::env::temp_dir().join(format!("dominion-host-test-{}",uuid::Uuid::new_v4()));std::fs::create_dir_all(&dir).unwrap();
    let config=Config{port:0,data_dir:dir.join("data"),workspace:dir.join("workspace"),web_dir:dir.join("web")};
    std::fs::create_dir_all(&config.data_dir).unwrap();std::fs::create_dir_all(&config.workspace).unwrap();
    let state=AppState::new(config.clone(),Store::open(&config.db_path()).unwrap(),Vault::open(&config.key_path()));
    let result=async {
        start(&state,"").await?;
        let fixture=PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/openclaw-plugin");
        state.store.put("plugin_install",&json!({"id":"test-progress","activities":[]})).unwrap();
        let installed=rpc(&state,"plugins.install",json!({"source":"local","path":fixture,"enable":true}),Some("test-progress"),Duration::from_secs(120)).await?;
        println!("Installed result: {}",installed);
        assert!(state.store.get("plugin_install","test-progress").unwrap()["activities"].as_array().unwrap().iter().any(|a|a["stage"]=="runtime"&&a["status"]=="completed"),"real runtime progress must be forwarded");
        let rows=rpc(&state,"plugins.list",json!({}),None,Duration::from_secs(30)).await?;
        assert!(rows["plugins"].as_array().unwrap().iter().any(|p|p["id"]=="dominion-host-fixture"&&p["enabled"]==true));
        let catalog=rpc(&state,"tools.catalog",json!({}),None,Duration::from_secs(30)).await?;
        if !plugin_tools(&catalog).iter().any(|p|p["name"]=="dominion_fixture_echo"){return Err(format!("Fixture tool missing: {}",rpc(&state,"plugins.inspect",json!({"pluginId":"dominion-host-fixture"}),None,Duration::from_secs(30)).await?));}
        let output=invoke(&state,&json!({"tool":"dominion_fixture_echo","arguments":{"text":"from-rust"}})).await?;
        println!("Tool result: {}",output);
        assert!(output.to_string().contains("from-rust"));
        let service=rpc(&state,"dominion.fixture.status",json!({}),None,Duration::from_secs(20)).await?;
        assert_eq!(service["running"],true,"service must be started by the real upstream loader");
        assert!(service["hookCalls"].as_u64().unwrap_or(0)>0,"real tool hook must run");
        manage(&state,&json!({"action":"configure","pluginId":"dominion-host-fixture","config":{"prefix":"configured:"}})).await?;
        let mut configured=false;
        for _ in 0..20{
            if let Ok(value)=invoke(&state,&json!({"tool":"dominion_fixture_echo","arguments":{"text":"after-config"}})).await{if value.to_string().contains("configured:after-config"){configured=true;break;}}
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        assert!(configured,"the real SDK config reload must reach tool execution");
        rpc(&state,"plugins.setEnabled",json!({"pluginId":"dominion-host-fixture","enabled":false}),None,Duration::from_secs(60)).await?;
        assert!(invoke(&state,&json!({"tool":"dominion_fixture_echo","arguments":{"text":"must not run"}})).await.is_err());
        rpc(&state,"plugins.uninstall",json!({"pluginId":"dominion-host-fixture"}),None,Duration::from_secs(60)).await?;
        Ok::<(),String>(())
    }.await;
    let _=stop(&state).await;
    if result.is_err(){if let Ok(log)=std::fs::read_to_string(dir.join("data/plugin-host/profile/host.log")){println!("Host log tail: {}",log.chars().rev().take(6000).collect::<String>().chars().rev().collect::<String>());}}
    let _=std::fs::remove_dir_all(&dir);result.unwrap();
}
