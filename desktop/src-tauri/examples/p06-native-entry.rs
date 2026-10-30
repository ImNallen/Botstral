#[cfg(target_os = "macos")]
#[path = "../src/protected_input.rs"]
mod protected_input;

#[cfg(target_os = "macos")]
fn main() {
    use botstral_link::{protected::*, *};
    use objc2::{MainThreadMarker, Message, rc::Retained};
    use objc2_app_kit::{NSApplication, NSSecureTextField, NSView};
    use objc2_foundation::NSString;
    use std::{cell::RefCell, time::Duration};

    thread_local! { static OBSERVED: RefCell<Vec<Retained<NSSecureTextField>>> = const { RefCell::new(Vec::new()) }; }
    fn fields(view: &NSView, found: &mut Vec<Retained<NSSecureTextField>>) {
        if let Some(field) = view.downcast_ref::<NSSecureTextField>() {
            found.push(field.retain());
        }
        for child in view.subviews() {
            fields(&child, found);
        }
    }
    let mtm = MainThreadMarker::new().expect("native fixture main thread");
    let app = NSApplication::sharedApplication(mtm);
    app.finishLaunching();
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(30));
        eprintln!("Native fixture deadline");
        std::process::exit(1);
    });
    for scenario in ["save", "cancel", "generation", "disconnect", "unlock"] {
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(250));
            dispatch2::DispatchQueue::main().exec_async(move || {
                let mtm = MainThreadMarker::new().unwrap();
                let app = NSApplication::sharedApplication(mtm);
                let mut found = Vec::new();
                for window in app.windows() {
                    if window.isVisible()
                        && let Some(view) = window.contentView()
                    {
                        fields(&view, &mut found);
                    }
                }
                assert_eq!(found.len(), if scenario == "unlock" { 1 } else { 3 });
                for field in &found {
                    field.setStringValue(&NSString::from_str("synthetic native input"));
                }
                OBSERVED.with(|observed| *observed.borrow_mut() = found);
                match scenario {
                    "generation" => protected_input::cancel_unless(2, true),
                    "disconnect" => protected_input::cancel_unless(1, false),
                    "cancel" => app.stopModalWithCode(1001),
                    _ => app.stopModalWithCode(1000),
                }
            });
        });
        let target = (scenario != "unlock").then(|| CredentialWriteTarget::Create {
            metadata: LoginMetadata {
                label: "Native fixture".into(),
                allowed_origins: vec!["https://example.test".into()],
                bots: BotAccess::AllOwnerBots,
                password_policy: PasswordPolicy::EveryFill,
            },
        });
        let result = protected_input::collect(1, target);
        match (scenario, result) {
            ("save", Ok(ProtectedOwnerOperation::Write { values, .. })) => assert!(
                values.username == "synthetic native input"
                    && values.password == "synthetic native input"
                    && values.totp_seed == "synthetic native input"
            ),
            ("unlock", Ok(ProtectedOwnerOperation::Unlock { passphrase })) => {
                assert!(passphrase.as_str() == "synthetic native input")
            }
            ("cancel" | "generation" | "disconnect", Err(_)) => {}
            _ => panic!("native entry result mismatch"),
        }
        OBSERVED.with(|observed| {
            assert!(
                observed
                    .borrow()
                    .iter()
                    .all(|field| field.stringValue().is_empty())
            );
            observed.borrow_mut().clear();
        });
        println!("native {scenario}: secure controls cleared");
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("Native entry requires macOS.");
    std::process::exit(1);
}
