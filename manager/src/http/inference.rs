use super::*;
use botstral_link::*;

pub(super) fn router() -> Router<Manager> {
    Router::new()
        .route("/v1/inference-configurations", put(configure))
        .route("/v1/inference-configurations/{bot}", get(configuration))
        .route("/v1/inference-configurations/verify", post(verify))
        .route("/v1/inferences", post(submit))
        .route("/v1/inferences/{id}", get(inference))
        .route("/v1/inferences/{id}/cancel", post(cancel))
}

async fn configure(
    State(m): State<Manager>,
    h: HeaderMap,
    StrictJson(body): StrictJson<ConfigureInference>,
) -> ApiResult<Json<InferenceConfiguration>> {
    Ok(Json(m.configure_inference(token(&h)?, body).await?))
}
async fn configuration(
    State(m): State<Manager>,
    h: HeaderMap,
    Path(bot): Path<BotId>,
) -> ApiResult<Json<InferenceConfiguration>> {
    Ok(Json(m.inference_configuration(token(&h)?, bot).await?))
}
async fn verify(
    State(m): State<Manager>,
    h: HeaderMap,
    StrictJson(body): StrictJson<VerifyInference>,
) -> ApiResult<Json<InferenceConfiguration>> {
    Ok(Json(m.verify_inference(token(&h)?, body).await?))
}
async fn submit(
    State(m): State<Manager>,
    h: HeaderMap,
    StrictJson(body): StrictJson<SubmitInference>,
) -> ApiResult<Json<InferenceView>> {
    Ok(Json(m.submit_inference(token(&h)?, body).await?))
}
async fn inference(
    State(m): State<Manager>,
    h: HeaderMap,
    Path(id): Path<InferenceId>,
) -> ApiResult<Json<InferenceView>> {
    Ok(Json(m.inference(token(&h)?, id).await?))
}
async fn cancel(
    State(m): State<Manager>,
    h: HeaderMap,
    Path(id): Path<InferenceId>,
    StrictJson(_): StrictJson<Empty>,
) -> ApiResult<Json<InferenceView>> {
    Ok(Json(m.cancel_inference(token(&h)?, id).await?))
}
