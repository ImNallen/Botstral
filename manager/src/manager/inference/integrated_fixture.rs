use super::*;
use std::os::unix::fs::PermissionsExt;

pub(super) fn response(body: &Value, path: &str) -> String {
    let config: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let mut value: Value = serde_json::from_str(COMPLETION).unwrap();
    if let Some(summary) = config["compaction_summary"].as_str() {
        if body["tools"]
            .as_array()
            .is_some_and(|tools| tools.iter().any(|tool| tool["function"]["name"] == "vibe"))
            && !body["messages"]
                .as_array()
                .unwrap()
                .iter()
                .any(|m| m["role"] == "tool")
        {
            value["choices"][0]["finish_reason"] = json!("tool_calls");
            value["choices"][0]["message"] = json!({"role":"assistant","content":null,"tool_calls":[{"id":"recorded-compaction-launch","type":"function","function":{"name":"vibe","arguments":json!({"prompt":"Retain the acceptance fixture context.","session":{"type":"new"}}).to_string()}}]});
        } else {
            let primary = body["tools"].as_array().is_some_and(|tools| {
                tools
                    .iter()
                    .any(|tool| tool["function"]["name"] == "read_file")
            });
            value["choices"][0]["message"]["content"] = if config["compaction_mode"] == "empty"
                || (config["compaction_mode"] == "fallback" && primary)
            {
                json!("The recorded provider omitted its summary.")
            } else {
                json!(format!("<summary>{summary}</summary>"))
            };
        }
        return value.to_string();
    }
    let done = body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|m| m["role"] == "tool");
    if !done {
        let name = config["tool"].as_str().expect("recorded connector name");
        assert!(
            body["tools"]
                .as_array()
                .unwrap()
                .iter()
                .any(|t| t["function"]["name"] == name)
        );
        value["choices"][0]["finish_reason"] = json!("tool_calls");
        value["choices"][0]["message"] = json!({"role":"assistant","content":null,"tool_calls":[{"id":"m06-recorded-connector","type":"function","function":{"name":name,"arguments":config["arguments"].to_string()}}]});
    }
    value.to_string()
}

#[tokio::test]
#[ignore = "Requires manager/tests/m06_integrated.py and isolated Linux VMs."]
async fn m06_integrated_remote_fixture() {
    let root = PathBuf::from(std::env::var_os("BOTSTRAL_M06_FIXTURE_ROOT").unwrap());
    let local = crate::LocalState::open(&root.join("manager-state")).unwrap();
    let fixture = Fixture::start().await;
    *fixture.state.production.lock().unwrap() = Some(json!({"m06":root.join("recorded.json")}));
    let mut app = App::start(local.directory().into(), &fixture).await;
    let identity = root.join("fixture-identity.json");
    if identity.exists() {
        app.bot = serde_json::from_slice(&std::fs::read(&identity).unwrap()).unwrap();
    } else {
        app.initialize().await;
        app.call(
            "PUT",
            &format!("/v1/bots/{}/permission", app.bot),
            json!({"expected_revision":1,"allow":true}),
            200,
        )
        .await;
        std::fs::write(&identity, serde_json::to_vec(&app.bot).unwrap()).unwrap();
        std::fs::set_permissions(&identity, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    let config: Value =
        serde_json::from_slice(&std::fs::read(root.join("config.json")).unwrap()).unwrap();
    let listener = crate::public_http::PublicListener::bind(crate::PublicTls {
        listen: config["public"].as_str().unwrap().parse().unwrap(),
        certificate: root.join("manager.pem"),
        key: root.join("manager.key"),
        client_ca: root.join("ca.pem"),
    })
    .await
    .unwrap();
    let public = listener.local_addr().unwrap();
    let (stop, stopped) = watch::channel(false);
    let server = tokio::spawn(listener.serve(app.manager.clone(), stopped));
    let path = root.join("fixture.json");
    std::fs::write(
        &path,
        json!({"local":app.address.trim_start_matches("http://"),"public":public,"bot":app.bot})
            .to_string(),
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    while !root.join("stop").exists() {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    stop.send_replace(true);
    server.await.unwrap().unwrap();
    app.shutdown().await;
}
