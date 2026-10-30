use std::collections::BTreeSet;

#[test]
fn packaged_main_window_can_invoke_registered_application_commands() {
    let shell = include_str!("../src/shell.rs");
    let build = include_str!("../build.rs");
    let capability: serde_json::Value =
        serde_json::from_str(include_str!("../capabilities/main.json")).unwrap();
    let permissions: BTreeSet<_> = capability["permissions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap())
        .collect();
    let mut command = false;
    for line in shell.lines().map(str::trim) {
        if line == "#[tauri::command]" {
            command = true;
        } else if command {
            let declaration = line.strip_prefix("async ").unwrap_or(line);
            let name = declaration
                .strip_prefix("fn ")
                .unwrap()
                .split('(')
                .next()
                .unwrap();
            assert!(
                build.contains(&format!("\"{name}\",")),
                "{name} is missing from the application manifest"
            );
            let permission = format!("allow-{}", name.replace('_', "-"));
            assert!(
                permissions.contains(permission.as_str()),
                "{name} cannot be invoked from the main window"
            );
            command = false;
        }
    }
}
