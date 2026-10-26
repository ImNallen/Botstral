use super::*;
use botstral_link::*;

macro_rules! owner_method {
    ($name:ident ($($arg:ident: $ty:ty),*) -> $out:ty) => {
        pub async fn $name(&self, token: SessionToken, $($arg: $ty),*) -> Result<$out, OperationError> {
            self.queue.call(false, move |s| s.$name(token, $($arg),*)).await
        }
    };
}
impl Manager {
    owner_method!(submit_task(input: SubmitTask) -> TaskView);
    owner_method!(task(id: TaskId) -> TaskView);
    owner_method!(cancel_task(id: TaskId) -> TaskView);
    owner_method!(grant_approval(id: TaskId, expires_at: i64) -> TaskView);
    owner_method!(revoke_approval(id: TaskId) -> TaskView);
    owner_method!(set_permission(bot: BotId, expected_revision: u64, allow: bool) -> Permission);
    owner_method!(conversation(input: Conversation) -> Conversation);
    owner_method!(append_message(input: ConversationMessage) -> ConversationMessage);
    owner_method!(resume(cursor: u64, limit: u16) -> EventReplay);
    owner_method!(write_document(input: WriteDocument) -> Document);
    owner_method!(search_documents(bot: BotId, query: String) -> Vec<Document>);
    owner_method!(rebuild_documents() -> Vec<Document>);
}
