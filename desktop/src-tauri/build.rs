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
        ]),
    ))
    .expect("desktop build configuration");
}
