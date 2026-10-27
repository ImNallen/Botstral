use botstral_link::*;

#[test]
fn owner_computer_command_is_first_class_and_strict() {
    let value = serde_json::json!({"type":"computer", "spec": {
        "account":{"uid":1000,"gid":1000},
        "runtime": {"sha256":"a".repeat(64)},
        "deadline":4000000000i64,
        "limits":{"runtime_seconds":60,"output_bytes":1024,"processes":128,"memory_bytes":1073741824u64},
        "viewport":{"width":800,"height":600},
        "operations":[{"type":"navigate","url":"https://example.org/"},
                      {"type":"capture_text","selector":"body","output":"text"}],
        "outputs":[{"slot":"text","name":"body.txt","media_type":"text_plain","max_bytes":4096}]
    }});
    let work: WorkSpec =
        serde_json::from_value(value.clone()).expect("owner API must accept typed Computer work");
    assert_eq!(serde_json::to_value(work).unwrap(), value);
    let mut invalid = value;
    invalid["spec"]["operations"][0] =
        serde_json::json!({"type":"evaluate","script":"arbitrary()"});
    assert!(serde_json::from_value::<WorkSpec>(invalid).is_err());
}
