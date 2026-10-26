use super::*;

fn epoch() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

#[tokio::test]
#[ignore = "Runs the opt-in production process contract on Linux with a systemd user manager."]
async fn w01_production_contract() {
    let mut f = Fixture::new().await;
    let session = f.durable_session().await;
    let deadline = epoch() + 120;
    let request = json!({
        "id":TaskId::new(),"session_id":session.id,"conversation_id":null,
        "settings":{"model":"unavailable","max_turns":1,"max_tokens":1,
            "deadline":deadline,"max_cost_micros":0},
        "work":{"type":"process","spec":{
            "profile":"linux_systemd_v1", "account":{"uid":unsafe { libc::geteuid() },"gid":unsafe { libc::getegid() }},
            "executable":"/usr/bin/printf","args":["%s", "literal $HOME %n"],
            "environment":{},"cwd":"/tmp","deadline":deadline,
            "limits":{"runtime_seconds":30,"output_bytes":1024,"processes":32,"memory_bytes":268435456}
        }}
    });
    let submitted = f.owner("POST", "/v1/tasks", request).await;
    assert_eq!(
        submitted.0, 200,
        "W01 production process submission is accepted: {}",
        submitted.1
    );
    f.manager.stop().await;
}
