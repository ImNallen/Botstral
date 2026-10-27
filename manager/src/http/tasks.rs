use super::*;
use botstral_link::*;

pub(super) fn router() -> Router<Manager> {
    Router::new()
        .route("/v1/tasks", post(submit))
        .route("/v1/tasks/{id}", get(task))
        .route("/v1/tasks/{task}/artifacts/{id}", get(download))
        .route("/v1/tasks/{id}/cancel", post(cancel))
        .route("/v1/tasks/{id}/approval", put(grant).delete(revoke))
        .route("/v1/bots/{id}/permission", put(permission))
        .route("/v1/conversations", post(conversation))
        .route("/v1/messages", post(message))
        .route("/v1/events/{cursor}/{limit}", get(replay))
        .route("/v1/documents", put(document))
        .route("/v1/generated-memory", put(memory))
        .route("/v1/documents/rebuild", post(rebuild))
        .route("/v1/documents/search/{id}", post(search))
}
async fn submit(
    State(m): State<Manager>,
    h: HeaderMap,
    StrictJson(body): StrictJson<SubmitTask>,
) -> ApiResult<Json<TaskView>> {
    Ok(Json(m.submit_task(token(&h)?, body).await?))
}
async fn task(
    State(m): State<Manager>,
    h: HeaderMap,
    Path(id): Path<TaskId>,
) -> ApiResult<Json<TaskView>> {
    Ok(Json(m.task(token(&h)?, id).await?))
}
async fn cancel(
    State(m): State<Manager>,
    h: HeaderMap,
    Path(id): Path<TaskId>,
    StrictJson(_): StrictJson<Empty>,
) -> ApiResult<Json<TaskView>> {
    Ok(Json(m.cancel_task(token(&h)?, id).await?))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Grant {
    expires_at: i64,
}
async fn grant(
    State(m): State<Manager>,
    h: HeaderMap,
    Path(id): Path<TaskId>,
    StrictJson(body): StrictJson<Grant>,
) -> ApiResult<Json<TaskView>> {
    Ok(Json(
        m.grant_approval(token(&h)?, id, body.expires_at).await?,
    ))
}
async fn revoke(
    State(m): State<Manager>,
    h: HeaderMap,
    Path(id): Path<TaskId>,
    StrictJson(_): StrictJson<Empty>,
) -> ApiResult<Json<TaskView>> {
    Ok(Json(m.revoke_approval(token(&h)?, id).await?))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Policy {
    expected_revision: u64,
    allow: bool,
}
async fn permission(
    State(m): State<Manager>,
    h: HeaderMap,
    Path(id): Path<BotId>,
    StrictJson(body): StrictJson<Policy>,
) -> ApiResult<Json<Permission>> {
    Ok(Json(
        m.set_permission(token(&h)?, id, body.expected_revision, body.allow)
            .await?,
    ))
}
async fn conversation(
    State(m): State<Manager>,
    h: HeaderMap,
    StrictJson(body): StrictJson<Conversation>,
) -> ApiResult<Json<Conversation>> {
    Ok(Json(m.conversation(token(&h)?, body).await?))
}
async fn message(
    State(m): State<Manager>,
    h: HeaderMap,
    StrictJson(body): StrictJson<ConversationMessage>,
) -> ApiResult<Json<ConversationMessage>> {
    Ok(Json(m.append_message(token(&h)?, body).await?))
}
async fn replay(
    State(m): State<Manager>,
    h: HeaderMap,
    Path((cursor, limit)): Path<(u64, u16)>,
) -> ApiResult<Json<EventReplay>> {
    Ok(Json(m.resume(token(&h)?, cursor, limit).await?))
}
async fn document(
    State(m): State<Manager>,
    h: HeaderMap,
    StrictJson(body): StrictJson<WriteDocument>,
) -> ApiResult<Json<Document>> {
    Ok(Json(m.write_document(token(&h)?, body).await?))
}
async fn memory(
    State(m): State<Manager>,
    h: HeaderMap,
    StrictJson(body): StrictJson<WriteDocument>,
) -> ApiResult<Json<Document>> {
    if body.kind != DocumentKind::Memory {
        return Err(OperationError::InvalidInput.into());
    }
    Ok(Json(m.write_document(token(&h)?, body).await?))
}
async fn rebuild(
    State(m): State<Manager>,
    h: HeaderMap,
    StrictJson(_): StrictJson<Empty>,
) -> ApiResult<Json<Vec<Document>>> {
    Ok(Json(m.rebuild_documents(token(&h)?).await?))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Search {
    query: String,
}
async fn search(
    State(m): State<Manager>,
    h: HeaderMap,
    Path(id): Path<BotId>,
    StrictJson(body): StrictJson<Search>,
) -> ApiResult<Json<Vec<Document>>> {
    Ok(Json(m.search_documents(token(&h)?, id, body.query).await?))
}

async fn download(
    State(m): State<Manager>,
    h: HeaderMap,
    Path((task, id)): Path<(TaskId, ArtifactId)>,
) -> ApiResult<Response> {
    let (file, declaration, length) = m.download_artifact(token(&h)?, task, id).await?;
    use tokio::io::AsyncReadExt;
    let stream = tokio_util::io::ReaderStream::new(tokio::fs::File::from_std(file).take(length));
    let mut response = Response::new(axum::body::Body::from_stream(stream));
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(match declaration.media_type {
            MediaType::Png => "image/png",
            MediaType::TextPlain => "text/plain; charset=utf-8",
            MediaType::OctetStream => "application/octet-stream",
        }),
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename=\"{}\"", declaration.name))
            .map_err(|_| OperationError::Internal)?,
    );
    headers.insert(
        header::CONTENT_LENGTH,
        HeaderValue::from_str(&length.to_string()).map_err(|_| OperationError::Internal)?,
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    Ok(response)
}
