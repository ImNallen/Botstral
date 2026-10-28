use super::*;
use botstral_link::*;

pub(super) fn router() -> Router<Manager> {
    Router::new()
        .route("/v1/connectors", get(list).post(install))
        .route("/v1/connectors/discover", post(discover))
        .route("/v1/connectors/{id}", get(view))
        .route("/v1/connectors/{id}/secret", put(secret))
        .route("/v1/connectors/{id}/reconnect", post(reconnect))
        .route("/v1/connectors/{id}/revoke", post(revoke))
        .route("/v1/connectors/{id}/audit/{after}", get(audit))
        .route("/v1/bots/{bot}/connectors/{id}", get(grants))
        .route("/v1/bots/{bot}/connectors", put(set_grants))
        .route("/v1/bots/{bot}/connector-tools", get(tools))
        .route("/v1/tasks/{task}/connector-calls", get(calls))
}
async fn install(
    State(m): State<Manager>,
    h: HeaderMap,
    StrictJson(input): StrictJson<InstallConnection>,
) -> ApiResult<Json<ConnectionView>> {
    Ok(Json(m.install_connection(token(&h)?, input).await?))
}
async fn list(State(m): State<Manager>, h: HeaderMap) -> ApiResult<Json<Vec<ConnectionView>>> {
    Ok(Json(m.connections(token(&h)?).await?))
}
async fn view(
    State(m): State<Manager>,
    h: HeaderMap,
    Path(id): Path<ConnectionId>,
) -> ApiResult<Json<ConnectionView>> {
    Ok(Json(m.connection(token(&h)?, id).await?))
}
async fn secret(
    State(m): State<Manager>,
    h: HeaderMap,
    Path(id): Path<ConnectionId>,
    StrictJson(input): StrictJson<ReplaceConnectionSecret>,
) -> ApiResult<Json<ConnectionView>> {
    Ok(Json(
        m.replace_connection_secret(token(&h)?, id, input).await?,
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Expected {
    expected_revision: u64,
}
async fn reconnect(
    State(m): State<Manager>,
    h: HeaderMap,
    Path(id): Path<ConnectionId>,
    StrictJson(input): StrictJson<Expected>,
) -> ApiResult<Json<ConnectionView>> {
    Ok(Json(
        m.reconnect_connection(token(&h)?, id, input.expected_revision)
            .await?,
    ))
}
async fn revoke(
    State(m): State<Manager>,
    h: HeaderMap,
    Path(id): Path<ConnectionId>,
    StrictJson(input): StrictJson<Expected>,
) -> ApiResult<Json<ConnectionView>> {
    Ok(Json(
        m.revoke_connection(token(&h)?, id, input.expected_revision)
            .await?,
    ))
}
async fn discover(
    State(m): State<Manager>,
    h: HeaderMap,
    StrictJson(input): StrictJson<DiscoverConnection>,
) -> ApiResult<Json<TaskView>> {
    Ok(Json(m.discover_connection(token(&h)?, input).await?))
}
async fn grants(
    State(m): State<Manager>,
    h: HeaderMap,
    Path((bot, id)): Path<(BotId, ConnectionId)>,
) -> ApiResult<Json<BotConnectorGrants>> {
    Ok(Json(m.connector_grants(token(&h)?, bot, id).await?))
}
async fn set_grants(
    State(m): State<Manager>,
    h: HeaderMap,
    Path(bot): Path<BotId>,
    StrictJson(input): StrictJson<SetConnectorGrants>,
) -> ApiResult<Json<BotConnectorGrants>> {
    Ok(Json(m.set_connector_grants(token(&h)?, bot, input).await?))
}
async fn tools(
    State(m): State<Manager>,
    h: HeaderMap,
    Path(bot): Path<BotId>,
) -> ApiResult<Json<Vec<ConnectorToolDefinition>>> {
    Ok(Json(m.connector_tools(token(&h)?, bot).await?))
}
async fn calls(
    State(m): State<Manager>,
    h: HeaderMap,
    Path(task): Path<TaskId>,
) -> ApiResult<Json<Vec<ConnectorCallView>>> {
    Ok(Json(m.connector_calls(token(&h)?, task).await?))
}
async fn audit(
    State(m): State<Manager>,
    h: HeaderMap,
    Path((id, after)): Path<(ConnectionId, u64)>,
) -> ApiResult<Json<Vec<ConnectorAudit>>> {
    Ok(Json(m.connector_audit(token(&h)?, id, after).await?))
}
