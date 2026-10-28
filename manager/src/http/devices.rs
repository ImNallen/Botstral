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
    server_name: String,
    ca_pem: String,
    certificate_pem: String,
}
async fn export_trust(
    State(manager): State<Manager>,
    headers: HeaderMap,
    StrictJson(body): StrictJson<ExportTrust>,
) -> ApiResult<Json<ManagerTrust>> {
    let identity = manager.manager_identity(token(&headers)?).await?;
    let certificates = rustls_pemfile::certs(&mut body.certificate_pem.as_bytes())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| OperationError::InvalidInput)?;
    let leaf = certificates.first().ok_or(OperationError::InvalidInput)?;
    let profile = ManagerTrust {
        manager: identity,
        endpoint: body.endpoint,
        server_name: body.server_name,
        ca_pem: body.ca_pem,
        leaf: CertificateFingerprint::from_der(leaf.as_ref()),
    };
    botstral_link::tls::validate_manager_trust(&profile, &certificates)
        .map_err(|_| OperationError::InvalidInput)?;
    Ok(Json(profile))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InvitationBody {
    request: PairingRequest,
    lifetime_seconds: u32,
}
async fn invite(
    State(manager): State<Manager>,
    headers: HeaderMap,
    StrictJson(body): StrictJson<InvitationBody>,
) -> ApiResult<Json<serde_json::Value>> {
    use age::secrecy::ExposeSecret;
    let grant = manager
        .invite_device(token(&headers)?, body.request, body.lifetime_seconds)
        .await?;
    Ok(Json(
        serde_json::json!({"manager":grant.manager,"fingerprint":grant.fingerprint,"role":grant.role,"expires_at":grant.expires_at,"invitation":grant.secret.expose_secret()}),
    ))
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
