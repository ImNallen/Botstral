use botstral_link::*;
use std::mem::size_of;

#[test]
fn admitted_snapshot_and_temporary_storage_fit_recovery_budgets() {
    assert_eq!(OWNER_SNAPSHOT_SERIALIZED_BOUND, 64_317_440);
    assert_eq!(OWNER_SNAPSHOT_ALLOCATION_BOUND, 67_272_704);
    const {
        assert!(OWNER_SNAPSHOT_SERIALIZED_BOUND < OWNER_TRANSFER_BYTES);
    }
    const {
        assert!(
            (MAX_OWNER_CONNECTIONS + 1) * OWNER_TRANSFER_BYTES
                + OWNER_SNAPSHOT_ALLOCATION_BOUND
                + 32 * 1024 * 1024
                < 1024 * 1024 * 1024
        );
    }
    const {
        assert!(OWNER_WORKSPACE_SERIALIZED_BOUND < OWNER_TRANSFER_BYTES);
    }
    const {
        assert!(
            (MAX_OWNER_CONNECTIONS + 1) * OWNER_TRANSFER_BYTES
                + OWNER_WORKSPACE_ALLOCATION_BOUND
                + 32 * 1024 * 1024
                < 1024 * 1024 * 1024
        );
    }
    assert!(size_of::<TaskView>() <= 4096);
    assert!(size_of::<ExecutionEvent>() <= 1024);
    assert!(size_of::<OutputView>() <= 256);
    assert!(size_of::<OutputDeclaration>() <= 128);
    assert!(size_of::<ComputerOperation>() <= 64);
    assert!(size_of::<ProcessSpec>() <= 1024);
    assert!(size_of::<InferenceView>() <= 2048);
    assert!(size_of::<InferenceConfiguration>() <= 1024);
    assert!(size_of::<ConversationMessage>() <= 256);
    assert!(size_of::<SourceReference>() <= 64);
    assert!(size_of::<Document>() <= 1024);
    assert!(size_of::<RunSummary>() <= 2048);
    assert!(size_of::<Permission>() <= 128);
    assert!(size_of::<Conversation>() <= 128);
    println!(
        "snapshot serialized={} decoded={} owner recovery peak<1GiB client temporary storage<=161MiB",
        OWNER_SNAPSHOT_SERIALIZED_BOUND, OWNER_SNAPSHOT_ALLOCATION_BOUND
    );
}
