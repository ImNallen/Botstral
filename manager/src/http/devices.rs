use super::*;
use botstral_link::*;

pub(super) fn router() -> Router<Manager> {
    Router::new()
        .route("/v1/manager-identity", get(identity))
        .route("/v1/device-trust", post(export_trust))
        .route("/v1/device-invitations", post(invite))
        .route("/v1/devices", get(devices))
        .route("/v1/devices/{id}", delete(revoke))
}
async fn identity(
    State(manager): State<Manager>,
    headers: HeaderMap,
) -> ApiResult<Json<serde_json::Value>> {
    Ok(Json(
        serde_json::json!({"manager":manager.manager_identity(token(&headers)?).await?}),
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExportTrust {
    endpoint: String,
}
async fn export_trust(
    State(manager): State<Manager>,
    headers: HeaderMap,
    StrictJson(body): StrictJson<ExportTrust>,
) -> ApiResult<Json<ManagerTrust>> {
    let identity = manager.manager_identity(token(&headers)?).await?;
    let endpoint = botstral_link::ManagerEndpoint::parse(&body.endpoint)
        .map_err(|_| OperationError::InvalidInput)?;
    let pin = manager
        .join_pin(token(&headers)?)
        .await?
        .ok_or(OperationError::InvalidInput)?;
    Ok(Json(ManagerTrust {
        manager: identity,
        endpoint: endpoint.as_str().to_owned(),
        pin: pin.fingerprint().clone(),
    }))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InvitationBody {
    label: String,
    lifetime_seconds: u32,
}
async fn invite(
    State(manager): State<Manager>,
    headers: HeaderMap,
    StrictJson(body): StrictJson<InvitationBody>,
) -> ApiResult<Response> {
    use age::secrecy::ExposeSecret;
    let grant = manager
        .invite_device(token(&headers)?, body.label, body.lifetime_seconds)
        .await?;
    Ok((StatusCode::CREATED, Json(serde_json::json!({"manager":grant.manager,"role":grant.role,"expires_at":grant.expires_at,"invitation":grant.secret.expose_secret(),"code":grant.code}))).into_response())
}
async fn devices(
    State(manager): State<Manager>,
    headers: HeaderMap,
) -> ApiResult<Json<Vec<OwnerDevice>>> {
    Ok(Json(manager.owner_devices(token(&headers)?).await?))
}
async fn revoke(
    State(manager): State<Manager>,
    headers: HeaderMap,
    IdPath(id): IdPath,
    StrictJson(_): StrictJson<Empty>,
) -> ApiResult<StatusCode> {
    manager
        .revoke_device(
            token(&headers)?,
            id.parse().map_err(|_| OperationError::InvalidInput)?,
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
