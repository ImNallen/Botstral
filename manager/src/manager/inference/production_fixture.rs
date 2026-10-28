use super::*;
use std::os::unix::fs::PermissionsExt;

pub(in super::super) fn response(body: &Value, config: &Value) -> String {
    let messages = body["messages"].as_array().unwrap();
    let prompt = messages.iter().find(|m| m["role"] == "user").unwrap()["content"]
        .as_str()
        .unwrap();
    let mut value: Value = serde_json::from_str(COMPLETION).unwrap();
    if messages.iter().any(|m| m["role"] == "tool") {
        value["choices"][0]["message"]["content"] =
            json!("The recorded provider received the production worker result.");
    } else if prompt.contains("browser") || prompt.contains("cancel") || prompt.contains("once") {
        let root = config["worker_root"].as_str().unwrap();
        let (name, arguments) = if prompt.contains("browser") {
            (
                "computer",
                json!({"operations":[
                {"type":"navigate","url":config["page"]},
                {"type":"fill","selector":"#value","text":"C02 production browser proof"},
                {"type":"click","selector":"#save"},
                {"type":"capture_text","selector":"#result","output":"text"},
                {"type":"write_file","path":format!("{root}/browser.txt"),"text":"C02 production file proof\n"},
                {"type":"screenshot","output":"image"}
            ],"outputs":["text","image"]}),
            )
        } else {
            let script = if prompt.contains("cancel") {
                format!(
                    "import os,subprocess,time,json; p=subprocess.Popen(['/usr/bin/sleep','120']); open('{root}/cancel-pids.json','w').write(json.dumps([os.getpid(),p.pid])); time.sleep(120)"
                )
            } else {
                format!("with open('{root}/effects.txt','a') as f: f.write('one effect\\n')")
            };
            (
                "process",
                json!({"executable":"/usr/bin/python3","args":["-c",script],"cwd":root}),
            )
        };
        value["choices"][0]["finish_reason"] = json!("tool_calls");
        value["choices"][0]["message"] = json!({"role":"assistant","content":"Review this exact production action.","tool_calls":[{"id":"c02-production-tool","type":"function","function":{"name":name,"arguments":arguments.to_string()}}]});
    } else {
        value["choices"][0]["message"]["content"] =
            json!("C02 durable recorded assistant. No paid inference.");
    }
    value.to_string()
}

fn publish(root: &std::path::Path, name: &str, value: Value) {
    let path = root.join(name);
    std::fs::write(&path, value.to_string()).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
}

#[tokio::test]
#[ignore = "Requires the isolated two-VM C02 production acceptance driver."]
async fn c02_production_remote_fixture() {
    let root = PathBuf::from(std::env::var_os("BOTSTRAL_C02_FIXTURE_ROOT").unwrap());
    let config: Value =
        serde_json::from_slice(&std::fs::read(root.join("config.json")).unwrap()).unwrap();
    let local = crate::LocalState::open(&root.join("manager-state")).unwrap();
    let fixture = Fixture::start().await;
    *fixture.state.production.lock().unwrap() = Some(config.clone());
    let mut app = App::start(local.directory().into(), &fixture).await;
    app.initialize().await;
    app.call(
        "PUT",
        &format!("/v1/bots/{}/permission", app.bot),
        json!({"expected_revision":1,"allow":true}),
        200,
    )
    .await;
    let bot = app.bot;
    let mut public = "0.0.0.0:0".parse().unwrap();
    let mut generation = 0;
    loop {
        let listener = crate::public_http::PublicListener::bind(crate::PublicTls {
            listen: public,
            certificate: root.join("manager.pem"),
            key: root.join("manager.key"),
            client_ca: root.join("ca.pem"),
        })
        .await
        .unwrap();
        public = listener.local_addr().unwrap();
        let (stop, stopped) = watch::channel(false);
        let manager = app.manager.clone();
        let server = tokio::spawn(async move { listener.serve(manager, stopped).await.unwrap() });
        publish(
            &root,
            "fixture.json",
            json!({"local":app.address.trim_start_matches("http://"),"public":public,"password":PASSWORD,"bot":bot,"generation":generation,"provider_calls":fixture.chats()}),
        );
        while !root.join("stop").exists() && !root.join("restart").exists() {
            publish(&root, "provider-count.json", json!(fixture.chats()));
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        stop.send_replace(true);
        server.await.unwrap();
        app.shutdown().await;
        if root.join("stop").exists() {
            break;
        }
        std::fs::remove_file(root.join("restart")).unwrap();
        app = App::start(local.directory().into(), &fixture).await;
        app.bot = bot;
        app.call(
            "POST",
            "/v1/vault/unlock",
            json!({"passphrase":PASSPHRASE}),
            204,
        )
        .await;
        app.verify(1, 200).await;
        generation += 1;
    }
}
