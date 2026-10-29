use botstral_link::{protected::*, *};

#[cfg(target_os = "macos")]
mod mac {
    use super::*;
    use objc2::{MainThreadMarker, MainThreadOnly, rc::Retained};
    use objc2_app_kit::{NSAlert, NSApplication, NSSecureTextField, NSTextField, NSView};
    use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
    use std::cell::RefCell;

    struct Entry {
        alert: Retained<NSAlert>,
        fields: Vec<Retained<NSSecureTextField>>,
        generation: u64,
    }
    thread_local! { static ENTRY: RefCell<Option<Entry>> = const { RefCell::new(None) }; }
    fn clear(entry: &Entry) {
        for field in &entry.fields {
            field.setStringValue(&NSString::new());
        }
    }
    pub(super) fn cancel_unless(generation: u64, online: bool) {
        ENTRY.with(|slot| {
            if let Some(entry) = slot
                .borrow()
                .as_ref()
                .filter(|entry| !online || entry.generation != generation)
            {
                clear(entry);
                let mtm = MainThreadMarker::new().expect("native input runs on the main thread");
                NSApplication::sharedApplication(mtm).abortModal();
                entry.alert.window().orderOut(None);
            }
        });
    }
    pub(super) fn collect(
        generation: u64,
        target: Option<CredentialWriteTarget>,
    ) -> Result<ProtectedOwnerOperation, String> {
        let mtm = MainThreadMarker::new().ok_or("native_thread")?;
        if ENTRY.with(|e| e.borrow().is_some()) {
            return Err("entry_busy".into());
        }
        let alert = NSAlert::new(mtm);
        let unlock = target.is_none();
        alert.setMessageText(&NSString::from_str(if unlock {
            "Unlock Vault"
        } else {
            "Protected login entry"
        }));
        alert.setInformativeText(&NSString::from_str(if unlock { "Enter the owner-provisioned Vault passphrase." } else { "Private fields stay in native controls. TOTP supports SHA-1, six digits and a 30-second period. Enter an uppercase unpadded Base32 seed or leave it empty." }));
        alert.addButtonWithTitle(&NSString::from_str(if unlock { "Unlock" } else { "Save" }));
        alert.addButtonWithTitle(&NSString::from_str("Cancel"));
        let labels: &[&str] = if unlock {
            &["Vault passphrase"]
        } else {
            &["Private username", "Password", "Optional TOTP seed"]
        };
        let view = NSView::initWithFrame(
            NSView::alloc(mtm),
            NSRect::new(
                NSPoint::new(0., 0.),
                NSSize::new(400., labels.len() as f64 * 64.),
            ),
        );
        let mut fields = Vec::new();
        for (index, label) in labels.iter().enumerate() {
            let y = (labels.len() - index - 1) as f64 * 64.;
            let caption = NSTextField::labelWithString(&NSString::from_str(label), mtm);
            caption.setFrame(NSRect::new(
                NSPoint::new(0., y + 30.),
                NSSize::new(400., 24.),
            ));
            view.addSubview(&caption);
            let field = NSSecureTextField::initWithFrame(
                NSSecureTextField::alloc(mtm),
                NSRect::new(NSPoint::new(0., y), NSSize::new(400., 26.)),
            );
            view.addSubview(&field);
            fields.push(field);
        }
        alert.setAccessoryView(Some(&view));
        ENTRY.with(|slot| {
            *slot.borrow_mut() = Some(Entry {
                alert: alert.clone(),
                fields,
                generation,
            })
        });
        let answer = alert.runModal();
        let entry = ENTRY
            .with(|slot| slot.borrow_mut().take())
            .ok_or("cancelled")?;
        let result = if answer == 1000 {
            match target {
                Some(target) => Ok(ProtectedOwnerOperation::Write {
                    target,
                    values: LoginValues {
                        username: entry.fields[0].stringValue().to_string(),
                        password: entry.fields[1].stringValue().to_string(),
                        totp_seed: entry.fields[2].stringValue().to_string(),
                    },
                }),
                None => Ok(ProtectedOwnerOperation::Unlock {
                    passphrase: zeroize::Zeroizing::new(entry.fields[0].stringValue().to_string()),
                }),
            }
        } else {
            Err("cancelled".into())
        };
        clear(&entry);
        alert.window().orderOut(None);
        result
    }
}

pub(crate) fn cancel_unless(generation: u64, online: bool) {
    #[cfg(target_os = "macos")]
    mac::cancel_unless(generation, online);
    #[cfg(not(target_os = "macos"))]
    let _ = (generation, online);
}
pub(crate) fn collect(
    generation: u64,
    target: Option<CredentialWriteTarget>,
) -> Result<ProtectedOwnerOperation, String> {
    #[cfg(target_os = "macos")]
    return mac::collect(generation, target);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (generation, target);
        Err("protected_native_entry_unsupported".into())
    }
}
