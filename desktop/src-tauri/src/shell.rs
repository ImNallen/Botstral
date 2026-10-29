use crate::local::{ErrorCode, Failure, LocalManager, Snapshot};
use crate::remote::{IntentCommand, RemoteService, RemoteSnapshot, SendInput};
use serde::Serialize;
use sha2::Digest;
use std::sync::Arc;
use tauri::Emitter;
use tauri::{
    Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
    menu::{Menu, MenuItem},
    tray::{TrayIconBuilder, TrayIconEvent},
};
use tauri_plugin_deep_link::DeepLinkExt;
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_notification::{NotificationExt, PermissionState};
use zeroize::Zeroizing;

fn allowed_page(url: &tauri::Url) -> bool {
    let root = matches!(url.path(), "" | "/" | "/index.html");
    root && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && ((url.scheme() == "tauri" && url.host_str() == Some("localhost"))
            || (url.scheme() == "http"
                && url.host_str() == Some("tauri.localhost")
                && url.port().is_none())
            || (cfg!(debug_assertions)
                && url.scheme() == "http"
                && url.host_str() == Some("127.0.0.1")
                && url.port() == Some(1420)))
}

#[derive(Serialize)]
#[serde(untagged)]
enum CommandFailure {
    Access { code: AccessCode },
    Operation(Failure),
}
#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum AccessCode {
    Unauthorized,
}

fn denied() -> CommandFailure {
    CommandFailure::Access {
        code: AccessCode::Unauthorized,
    }
}

fn authorize(window: &WebviewWindow) -> Result<(), CommandFailure> {
    if window.label() == "main" && window.url().is_ok_and(|url| allowed_page(&url)) {
        return Ok(());
    }
    Err(denied())
}

#[tauri::command]
fn inspect(
    window: WebviewWindow,
    service: tauri::State<'_, LocalManager>,
) -> Result<Snapshot, CommandFailure> {
    authorize(&window)?;
    Ok(service.inspect())
}
#[tauri::command]
async fn sign_in(
    window: WebviewWindow,
    service: tauri::State<'_, LocalManager>,
    endpoint: String,
    password: String,
) -> Result<Snapshot, CommandFailure> {
    let password = Zeroizing::new(password);
    authorize(&window)?;
    service
        .sign_in(endpoint, password)
        .await
        .map_err(CommandFailure::Operation)
}
#[tauri::command]
async fn refresh(
    window: WebviewWindow,
    service: tauri::State<'_, LocalManager>,
) -> Result<Snapshot, CommandFailure> {
    authorize(&window)?;
    service.refresh().await.map_err(CommandFailure::Operation)
}
#[tauri::command]
async fn create_bot(
    window: WebviewWindow,
    service: tauri::State<'_, LocalManager>,
    id: String,
    name: String,
) -> Result<Snapshot, CommandFailure> {
    authorize(&window)?;
    let id = id.parse().map_err(|_| {
        CommandFailure::Operation(Failure {
            code: ErrorCode::InvalidInput,
            snapshot: Box::new(service.inspect()),
        })
    })?;
    service
        .create_bot(id, name)
        .await
        .map_err(CommandFailure::Operation)
}
#[tauri::command]
async fn sign_out(
    window: WebviewWindow,
    service: tauri::State<'_, LocalManager>,
    remote: tauri::State<'_, RemoteService>,
) -> Result<Snapshot, CommandFailure> {
    authorize(&window)?;
    let snapshot = service.sign_out();
    let result = remote.sign_out().await;
    if result.is_err() || remote.inspect().issue.is_some() {
        return Err(CommandFailure::Operation(Failure {
            code: ErrorCode::NativeStorage,
            snapshot: Box::new(snapshot),
        }));
    }
    Ok(snapshot)
}

#[tauri::command]
async fn remote_inspect(
    window: WebviewWindow,
    service: tauri::State<'_, RemoteService>,
) -> Result<RemoteSnapshot, String> {
    authorize(&window).map_err(|_| "unauthorized")?;
    service.restore().await
}
#[tauri::command]
async fn begin_pairing(
    window: WebviewWindow,
    service: tauri::State<'_, RemoteService>,
    label: String,
) -> Result<RemoteSnapshot, String> {
    authorize(&window).map_err(|_| "unauthorized")?;
    service.command(IntentCommand::Begin { label }).await
}
async fn pick_json<T: serde::de::DeserializeOwned>(app: &tauri::AppHandle) -> Result<T, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .add_filter("Pairing material", &["json"])
        .pick_file(move |path| {
            let _ = tx.send(path);
        });
    let path = rx
        .await
        .map_err(|_| "dialog")?
        .ok_or("cancelled")?
        .into_path()
        .map_err(|_| "invalid_input")?;
    let bytes = botstral_link::tls::private_bytes(&path).map_err(|_| "protected_file_required")?;
    if bytes.len() > 65536 {
        return Err("invalid_input".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| "invalid_input".into())
}
#[tauri::command]
async fn import_pairing(
    window: WebviewWindow,
    app: tauri::AppHandle,
    service: tauri::State<'_, RemoteService>,
) -> Result<RemoteSnapshot, String> {
    authorize(&window).map_err(|_| "unauthorized")?;
    service
        .command(IntentCommand::Stage(pick_json(&app).await?))
        .await
}
#[tauri::command]
async fn import_trust(
    window: WebviewWindow,
    app: tauri::AppHandle,
    service: tauri::State<'_, RemoteService>,
) -> Result<RemoteSnapshot, String> {
    authorize(&window).map_err(|_| "unauthorized")?;
    service
        .command(IntentCommand::StageTrust(pick_json(&app).await?))
        .await
}
#[tauri::command]
async fn pair_local(
    window: WebviewWindow,
    app: tauri::AppHandle,
    service: tauri::State<'_, RemoteService>,
    local: tauri::State<'_, LocalManager>,
) -> Result<RemoteSnapshot, String> {
    authorize(&window).map_err(|_| "unauthorized")?;
    let request = service.inspect().pairing.ok_or("begin_pairing_first")?;
    let bundle = local
        .invite_device(request, pick_json(&app).await?)
        .await
        .map_err(|_| "local_invitation_failed")?;
    service.command(IntentCommand::Stage(bundle)).await
}
#[tauri::command]
async fn confirm_pairing(
    window: WebviewWindow,
    service: tauri::State<'_, RemoteService>,
    candidate: uuid::Uuid,
) -> Result<RemoteSnapshot, String> {
    authorize(&window).map_err(|_| "unauthorized")?;
    service.command(IntentCommand::Confirm(candidate)).await
}
#[tauri::command]
async fn remote_refresh(
    window: WebviewWindow,
    service: tauri::State<'_, RemoteService>,
) -> Result<RemoteSnapshot, String> {
    authorize(&window).map_err(|_| "unauthorized")?;
    service.command(IntentCommand::Refresh).await
}
#[tauri::command]
async fn run_details(
    window: WebviewWindow,
    service: tauri::State<'_, RemoteService>,
    id: botstral_link::RunId,
) -> Result<RemoteSnapshot, String> {
    authorize(&window).map_err(|_| "unauthorized")?;
    service.command(IntentCommand::RunDetails(id)).await
}
#[tauri::command]
async fn routine_text(
    window: WebviewWindow,
    service: tauri::State<'_, RemoteService>,
    bot: botstral_link::BotId,
    text: String,
) -> Result<RemoteSnapshot, String> {
    authorize(&window).map_err(|_| "unauthorized")?;
    service
        .command(IntentCommand::RoutineText { bot, text })
        .await
}
#[tauri::command]
async fn routine_command(
    window: WebviewWindow,
    service: tauri::State<'_, RemoteService>,
    input: botstral_link::RoutineCommand,
) -> Result<RemoteSnapshot, String> {
    authorize(&window).map_err(|_| "unauthorized")?;
    service.command(IntentCommand::Routine(input)).await
}
#[tauri::command]
async fn create_memory(
    window: WebviewWindow,
    service: tauri::State<'_, RemoteService>,
    bot: botstral_link::BotId,
    source_message: botstral_link::MessageId,
    markdown: String,
) -> Result<RemoteSnapshot, String> {
    authorize(&window).map_err(|_| "unauthorized")?;
    service
        .command(IntentCommand::CreateMemory {
            bot,
            source_message,
            markdown,
        })
        .await
}
#[tauri::command]
async fn write_memory(
    window: WebviewWindow,
    service: tauri::State<'_, RemoteService>,
    bot: botstral_link::BotId,
    expected_revision: u64,
    markdown: String,
) -> Result<RemoteSnapshot, String> {
    authorize(&window).map_err(|_| "unauthorized")?;
    service
        .command(IntentCommand::WriteMemory {
            bot,
            expected_revision,
            markdown,
        })
        .await
}
#[tauri::command]
async fn save_routine(
    window: WebviewWindow,
    service: tauri::State<'_, RemoteService>,
    id: botstral_link::RoutineId,
    expected_revision: u64,
    definition_json: String,
) -> Result<RemoteSnapshot, String> {
    authorize(&window).map_err(|_| "unauthorized")?;
    if definition_json.len() > botstral_link::MAX_ROUTINE_BYTES {
        return Err("invalid_input".into());
    }
    let definition = serde_json::from_str(&definition_json).map_err(|_| "invalid_input")?;
    service
        .command(IntentCommand::Routine(botstral_link::RoutineCommand {
            id: botstral_link::RequestId::new(),
            action: botstral_link::RoutineAction::Replace {
                id,
                expected_revision,
                definition,
            },
        }))
        .await
}
#[tauri::command]
async fn routine_history(
    window: WebviewWindow,
    service: tauri::State<'_, RemoteService>,
    id: botstral_link::RoutineId,
    before: Option<botstral_link::OccurrenceId>,
) -> Result<RemoteSnapshot, String> {
    authorize(&window).map_err(|_| "unauthorized")?;
    service
        .command(IntentCommand::RoutineHistory { id, before })
        .await
}
#[tauri::command]
async fn routine_preview(
    window: WebviewWindow,
    service: tauri::State<'_, RemoteService>,
    trigger: botstral_link::RoutineTrigger,
    after: i64,
) -> Result<RemoteSnapshot, String> {
    authorize(&window).map_err(|_| "unauthorized")?;
    service
        .command(IntentCommand::RoutinePreview { trigger, after })
        .await
}
#[tauri::command]
async fn send_run(
    window: WebviewWindow,
    service: tauri::State<'_, RemoteService>,
    input: SendInput,
) -> Result<RemoteSnapshot, String> {
    authorize(&window).map_err(|_| "unauthorized")?;
    service.command(IntentCommand::Send(input)).await
}
#[tauri::command]
async fn open_session(
    window: WebviewWindow,
    service: tauri::State<'_, RemoteService>,
    bot: botstral_link::BotId,
    worker: botstral_link::WorkerId,
) -> Result<RemoteSnapshot, String> {
    authorize(&window).map_err(|_| "unauthorized")?;
    service
        .command(IntentCommand::OpenSession { bot, worker })
        .await
}
#[tauri::command]
async fn set_permission(
    window: WebviewWindow,
    service: tauri::State<'_, RemoteService>,
    bot: botstral_link::BotId,
    expected: u64,
    allow: bool,
) -> Result<RemoteSnapshot, String> {
    authorize(&window).map_err(|_| "unauthorized")?;
    service
        .command(IntentCommand::Permission {
            bot,
            expected,
            allow,
        })
        .await
}
#[tauri::command]
async fn review_approval(
    window: WebviewWindow,
    service: tauri::State<'_, RemoteService>,
    task: botstral_link::TaskId,
) -> Result<RemoteSnapshot, String> {
    authorize(&window).map_err(|_| "unauthorized")?;
    service.command(IntentCommand::Review(task)).await
}
#[tauri::command]
async fn approve_action(
    window: WebviewWindow,
    service: tauri::State<'_, RemoteService>,
    review: uuid::Uuid,
) -> Result<RemoteSnapshot, String> {
    authorize(&window).map_err(|_| "unauthorized")?;
    service.command(IntentCommand::Approve(review)).await
}
#[tauri::command]
async fn cancel_task(
    window: WebviewWindow,
    service: tauri::State<'_, RemoteService>,
    task: botstral_link::TaskId,
) -> Result<RemoteSnapshot, String> {
    authorize(&window).map_err(|_| "unauthorized")?;
    service.command(IntentCommand::CancelTask(task)).await
}
#[tauri::command]
async fn cancel_run(
    window: WebviewWindow,
    service: tauri::State<'_, RemoteService>,
    run: botstral_link::RunId,
) -> Result<RemoteSnapshot, String> {
    authorize(&window).map_err(|_| "unauthorized")?;
    service.command(IntentCommand::CancelRun(run)).await
}
#[tauri::command]
async fn revoke_approval(
    window: WebviewWindow,
    service: tauri::State<'_, RemoteService>,
    task: botstral_link::TaskId,
) -> Result<RemoteSnapshot, String> {
    authorize(&window).map_err(|_| "unauthorized")?;
    service.command(IntentCommand::RevokeApproval(task)).await
}
#[tauri::command]
async fn acknowledge_uncertainty(
    window: WebviewWindow,
    service: tauri::State<'_, RemoteService>,
    intent: uuid::Uuid,
) -> Result<RemoteSnapshot, String> {
    authorize(&window).map_err(|_| "unauthorized")?;
    service
        .command(IntentCommand::AcknowledgeUncertainty(intent))
        .await
}
#[tauri::command]
async fn remote_sign_out(
    window: WebviewWindow,
    service: tauri::State<'_, RemoteService>,
    local: tauri::State<'_, LocalManager>,
) -> Result<RemoteSnapshot, String> {
    authorize(&window).map_err(|_| "unauthorized")?;
    local.sign_out();
    service.sign_out().await
}
#[tauri::command]
async fn remote_lifecycle(
    window: WebviewWindow,
    service: tauri::State<'_, RemoteService>,
    suspended: bool,
) -> Result<RemoteSnapshot, String> {
    authorize(&window).map_err(|_| "unauthorized")?;
    service
        .command(if suspended {
            IntentCommand::Suspend
        } else {
            IntentCommand::Resume
        })
        .await
}
#[tauri::command]
async fn export_artifact(
    window: WebviewWindow,
    app: tauri::AppHandle,
    service: tauri::State<'_, RemoteService>,
    task: botstral_link::TaskId,
    artifact: botstral_link::ArtifactId,
) -> Result<bool, String> {
    authorize(&window).map_err(|_| "unauthorized")?;
    let generation = service.inspect().generation;
    let verified = service.artifact(task, artifact).await?;
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_file_name(&verified.name)
        .save_file(move |path| {
            let _ = tx.send(path);
        });
    let Some(path) = rx.await.map_err(|_| "dialog")? else {
        return Ok(false);
    };
    if generation != service.inspect().generation {
        return Err("stale".into());
    }
    crate::remote::save_artifact(&path.into_path().map_err(|_| "invalid_input")?, verified)?;
    Ok(true)
}
#[tauri::command]
async fn list_devices(
    window: WebviewWindow,
    service: tauri::State<'_, LocalManager>,
) -> Result<Vec<botstral_link::OwnerDevice>, String> {
    authorize(&window).map_err(|_| "unauthorized")?;
    service
        .devices()
        .await
        .map_err(|_| "local_administration_required".into())
}
#[tauri::command]
async fn revoke_device(
    window: WebviewWindow,
    service: tauri::State<'_, LocalManager>,
    device: botstral_link::DeviceId,
) -> Result<(), String> {
    authorize(&window).map_err(|_| "unauthorized")?;
    service
        .revoke_device(device)
        .await
        .map_err(|_| "revocation_unconfirmed".into())
}
#[tauri::command]
async fn issue_invitation(
    window: WebviewWindow,
    app: tauri::AppHandle,
    service: tauri::State<'_, LocalManager>,
    fingerprint: botstral_link::DeviceFingerprint,
    label: String,
) -> Result<bool, String> {
    authorize(&window).map_err(|_| "unauthorized")?;
    let bundle = service
        .invite_device(
            botstral_link::PairingRequest { fingerprint, label },
            pick_json(&app).await?,
        )
        .await
        .map_err(|_| "local_invitation_failed")?;
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_file_name("owner-invitation.json")
        .save_file(move |path| {
            let _ = tx.send(path);
        });
    let Some(path) = rx.await.map_err(|_| "dialog")? else {
        return Ok(false);
    };
    let bytes = serde_json::to_vec(&bundle).map_err(|_| "invalid_input")?;
    crate::remote::save_artifact(
        &path.into_path().map_err(|_| "invalid_input")?,
        crate::remote::VerifiedArtifact {
            name: "owner-invitation.json".into(),
            bytes,
        },
    )?;
    Ok(true)
}
#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum NotificationResult {
    Submitted,
    Denied,
    Unavailable,
}

#[tauri::command]
fn test_notification(
    window: WebviewWindow,
    app: tauri::AppHandle,
) -> Result<NotificationResult, CommandFailure> {
    authorize(&window)?;
    let notification = app.notification();
    let permission = notification
        .permission_state()
        .and_then(|permission| match permission {
            PermissionState::Prompt | PermissionState::PromptWithRationale => {
                notification.request_permission()
            }
            permission => Ok(permission),
        });
    match permission {
        Ok(PermissionState::Granted) => Ok(
            if notification
                .builder()
                .title("Botstral")
                .body("Your test notification. No bot activity occurred.")
                .show()
                .is_ok()
            {
                NotificationResult::Submitted
            } else {
                NotificationResult::Unavailable
            },
        ),
        Ok(PermissionState::Denied) => Ok(NotificationResult::Denied),
        _ => Ok(NotificationResult::Unavailable),
    }
}

fn show(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let service = app.state::<RemoteService>().inner().clone();
        tauri::async_runtime::spawn(async move {
            let _ = service.command(IntentCommand::Resume).await;
        });
        let _ = window.set_focus();
    }
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_deep_link::init())
        .manage(LocalManager::new().expect("local client initialization"))
        .invoke_handler(tauri::generate_handler![
            inspect,
            sign_in,
            refresh,
            create_bot,
            sign_out,
            test_notification,
            remote_inspect,
            begin_pairing,
            import_pairing,
            import_trust,
            pair_local,
            confirm_pairing,
            remote_refresh,
            send_run,
            run_details,
            routine_command,
            routine_text,
            save_routine,
            write_memory,
            create_memory,
            routine_history,
            routine_preview,
            open_session,
            set_permission,
            review_approval,
            approve_action,
            cancel_task,
            cancel_run,
            revoke_approval,
            remote_sign_out,
            acknowledge_uncertainty,
            remote_lifecycle,
            export_artifact,
            list_devices,
            revoke_device,
            issue_invitation
        ])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .setup(|app| {
            let root = app.path().app_data_dir()?.join("owner-v1");
            #[cfg(debug_assertions)]
            let root = std::env::var_os("BOTSTRAL_DESKTOP_PROFILE").map(std::path::PathBuf::from).unwrap_or(root);
            let slot = format!("{:x}", sha2::Sha256::digest(root.as_os_str().as_encoded_bytes()));
            let store = Arc::new(crate::keychain::NativeSecrets::new(&slot)?);
            let remote = RemoteService::new(root, store);
            app.manage(remote.clone());
            if let Some(urls) = app.deep_link().get_current()? {
                for url in urls {
                    if let Ok(bundle) = crate::remote::parse_pairing_link(url.as_str()) {
                        let service = remote.clone();
                        tauri::async_runtime::spawn(async move { let _ = service.command(IntentCommand::Stage(bundle)).await; });
                    }
                }
            }
            let link_service = remote.clone();
            app.deep_link().on_open_url(move |event| {
                for url in event.urls() {
                    if let Ok(bundle) = crate::remote::parse_pairing_link(url.as_str()) {
                        let service = link_service.clone();
                        tauri::async_runtime::spawn(async move { let _ = service.command(IntentCommand::Stage(bundle)).await; });
                    }
                }
            });
            let wake_service = remote.clone();
            tauri::async_runtime::spawn(async move {
                let mut previous = std::time::SystemTime::now();
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    let current = std::time::SystemTime::now();
                    if current.duration_since(previous).unwrap_or_default() > std::time::Duration::from_secs(10) && wake_service.inspect().status != "suspended" {
                        let _ = wake_service.command(IntentCommand::Suspend).await;
                        let _ = wake_service.command(IntentCommand::Resume).await;
                    }
                    previous = current;
                }
            });
            let mut changes = remote.observe();
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let _ = remote.restore().await;
                let mut cursor = None;
                while changes.changed().await.is_ok() {
                    let view = changes.borrow_and_update().clone();
                    if let Some(window) = handle.get_webview_window("main").filter(|w| w.url().is_ok_and(|u| allowed_page(&u))) {
                        let _ = window.emit_to(tauri::EventTarget::webview_window("main"), "workspace-changed", &view);
                    }
                    if let Some(image) = &view.workspace {
                        if cursor.is_some_and(|old| image.cursor > old) && view.status == "online" {
                            let _ = handle.notification().builder().title("Botstral activity").body("Workspace activity changed. Open Botstral to review current state.").show();
                        }
                        cursor = Some(image.cursor);
                    }
                }
            });
            let window =
                WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                    .title("Botstral")
                    .inner_size(1200.0, 800.0)
                    .min_inner_size(880.0, 620.0)
                    .on_navigation(allowed_page)
                    .on_new_window(|_, _| tauri::webview::NewWindowResponse::Deny);
            #[cfg(target_os = "macos")]
            let window = window
                .title_bar_style(tauri::TitleBarStyle::Overlay)
                .hidden_title(true)
                .traffic_light_position(tauri::LogicalPosition::new(16.0, 24.0));
            window.build()?;
            let show_item = MenuItem::with_id(app, "show", "Show Botstral", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "Quit Botstral", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_item, &quit_item])?;
            TrayIconBuilder::new()
                .icon(tauri::include_image!("icons/tray.png"))
                .icon_as_template(true)
                .tooltip("Botstral")
                .menu(&menu)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => show(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if matches!(event, TrayIconEvent::DoubleClick { .. }) {
                        show(tray.app_handle());
                    }
                })
                .build(app)?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("desktop application failed");
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejected_callers_receive_no_application_data() {
        assert_eq!(
            serde_json::to_value(denied()).unwrap(),
            serde_json::json!({"code":"unauthorized"})
        );
    }
    #[test]
    fn native_page_policy_rejects_remote_and_nested_content() {
        for url in [
            "https://example.com/",
            "tauri://example.com/",
            "tauri://localhost/remote",
            "tauri://localhost/?url=remote",
            "http://127.0.0.1:9999/",
            "http://tauri.localhost:1234/",
        ] {
            assert!(!allowed_page(&url.parse().unwrap()), "{url}");
        }
        assert!(
            allowed_page(&"tauri://localhost".parse().unwrap()),
            "Tauri's packaged index page must be admitted without a trailing slash"
        );
        assert!(allowed_page(&"tauri://localhost/".parse().unwrap()));
        assert!(allowed_page(
            &"http://tauri.localhost/index.html".parse().unwrap()
        ));
    }
}
