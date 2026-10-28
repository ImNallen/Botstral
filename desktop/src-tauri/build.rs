fn main() {
    #[cfg(feature = "native-ui")]
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "inspect",
            "sign_in",
            "refresh",
            "create_bot",
            "sign_out",
            "test_notification",
            "remote_inspect",
            "begin_pairing",
            "import_pairing",
            "import_trust",
            "pair_local",
            "confirm_pairing",
            "remote_refresh",
            "send_run",
            "open_session",
            "set_permission",
            "review_approval",
            "approve_action",
            "cancel_task",
            "cancel_run",
            "revoke_approval",
            "remote_sign_out",
            "acknowledge_uncertainty",
            "remote_lifecycle",
            "export_artifact",
            "list_devices",
            "revoke_device",
            "issue_invitation",
        ]),
    ))
    .expect("desktop build configuration");
}
