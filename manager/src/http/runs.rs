use super::*;
use botstral_link::*;

pub(super) fn router() -> Router<Manager> {
    Router::new()
        .route("/v1/runs", post(submit))
        .route("/v1/runs/{id}", get(run))
        .route("/v1/runs/{id}/cancel", post(cancel))
}
async fn submit(
    State(m): State<Manager>,
    h: HeaderMap,
    StrictJson(body): StrictJson<SubmitRun>,
) -> ApiResult<Json<RunView>> {
    Ok(Json(m.submit_run(token(&h)?, body).await?))
}
async fn run(
    State(m): State<Manager>,
    h: HeaderMap,
    Path(id): Path<RunId>,
) -> ApiResult<Json<RunView>> {
    Ok(Json(m.run(token(&h)?, id).await?))
}
async fn cancel(
    State(m): State<Manager>,
    h: HeaderMap,
    Path(id): Path<RunId>,
    StrictJson(_): StrictJson<Empty>,
) -> ApiResult<Json<RunView>> {
    Ok(Json(m.cancel_run(token(&h)?, id).await?))
}
