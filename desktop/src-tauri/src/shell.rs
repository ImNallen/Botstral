use crate::local::{ErrorCode, Failure, LocalManager, Snapshot};
use serde::Serialize;
use tauri::{
    Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
    menu::{Menu, MenuItem},
    tray::{TrayIconBuilder, TrayIconEvent},
};
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
) -> Result<Snapshot, CommandFailure> {
    authorize(&window)?;
    Ok(service.sign_out())
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
        let _ = window.set_focus();
    }
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .manage(LocalManager::new().expect("local client initialization"))
        .invoke_handler(tauri::generate_handler![
            inspect,
            sign_in,
            refresh,
            create_bot,
            sign_out,
            test_notification
        ])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .setup(|app| {
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
