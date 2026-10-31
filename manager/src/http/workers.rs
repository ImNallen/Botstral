use super::*;
use botstral_link::{ComputerSession, ComputerSessionId, WorkerId};

pub(super) fn router() -> Router<Manager> {
    Router::new()
        .route("/v1/worker-invitations", post(invite))
        .route("/v1/workers", get(workers))
        .route("/v1/workers/{id}", delete(revoke))
        .route("/v1/computer-sessions", post(open_session))
        .route("/v1/computer-sessions/{id}", get(session))
        .route("/v1/computer-sessions/{id}/owner", put(takeover))
        .route("/v1/worker-commands", post(command))
        .route("/v1/worker-input", post(input))
        .route("/v1/workers/{worker}/receipts/{command}", get(receipt))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InvitationBody {
    lifetime_seconds: u32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionBody {
    bot_id: BotId,
    worker_id: WorkerId,
}

async fn invite(
    State(manager): State<Manager>,
    headers: HeaderMap,
    StrictJson(body): StrictJson<InvitationBody>,
) -> ApiResult<Response> {
    use age::secrecy::ExposeSecret;
    let grant = manager
        .invite_worker(token(&headers)?, body.lifetime_seconds)
        .await?;
    Ok((StatusCode::CREATED, Json(serde_json::json!({"invitation":grant.secret.expose_secret(), "expires_at":grant.expires_at, "code":grant.code}))).into_response())
}
async fn workers(
    State(manager): State<Manager>,
    headers: HeaderMap,
) -> ApiResult<Json<Vec<WorkerSummary>>> {
    Ok(Json(manager.workers(token(&headers)?).await?))
}
async fn revoke(
    State(manager): State<Manager>,
    headers: HeaderMap,
    IdPath(id): IdPath,
    StrictJson(_): StrictJson<Empty>,
) -> ApiResult<StatusCode> {
    manager
        .revoke_worker(
            token(&headers)?,
            id.parse().map_err(|_| OperationError::InvalidInput)?,
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
async fn open_session(
    State(manager): State<Manager>,
    headers: HeaderMap,
    StrictJson(body): StrictJson<SessionBody>,
) -> ApiResult<Json<ComputerSession>> {
    Ok(Json(
        manager
            .open_computer_session(token(&headers)?, body.bot_id, body.worker_id)
            .await?,
    ))
}
async fn session(
    State(manager): State<Manager>,
    headers: HeaderMap,
    IdPath(id): IdPath,
) -> ApiResult<Json<ComputerSession>> {
    let id: ComputerSessionId = id.parse().map_err(|_| OperationError::InvalidInput)?;
    Ok(Json(manager.computer_session(token(&headers)?, id).await?))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OwnershipBody {
    expected_revision: botstral_link::Revision,
    owner: botstral_link::InputOwner,
}
async fn takeover(
    State(manager): State<Manager>,
    headers: HeaderMap,
    IdPath(id): IdPath,
    StrictJson(body): StrictJson<OwnershipBody>,
) -> ApiResult<Json<ComputerSession>> {
    Ok(Json(
        manager
            .set_input_owner(
                token(&headers)?,
                id.parse().map_err(|_| OperationError::InvalidInput)?,
                body.expected_revision,
                body.owner,
            )
            .await?,
    ))
}
async fn command(
    State(manager): State<Manager>,
    headers: HeaderMap,
    StrictJson(body): StrictJson<botstral_link::Command>,
) -> ApiResult<Json<DispatchReceipt>> {
    Ok(Json(manager.command(token(&headers)?, body).await?))
}
async fn input(
    State(manager): State<Manager>,
    headers: HeaderMap,
    StrictJson(body): StrictJson<OwnerInput>,
) -> ApiResult<Json<DispatchReceipt>> {
    Ok(Json(manager.input(token(&headers)?, body).await?))
}
async fn receipt(
    State(manager): State<Manager>,
    headers: HeaderMap,
    Path((worker, command)): Path<(WorkerId, botstral_link::CommandId)>,
) -> ApiResult<Json<DispatchReceipt>> {
    Ok(Json(
        manager.receipt(token(&headers)?, worker, command).await?,
    ))
}
