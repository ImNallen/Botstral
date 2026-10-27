use super::*;
use botstral_link::*;

impl State {
    pub(super) fn authorize_images(
        &self,
        request: &SubmitRun,
        references: &[ImageReference],
    ) -> Result<Vec<AuthorizedImage>, OperationError> {
        if references.is_empty() {
            return Ok(Vec::new());
        }
        let policy = request
            .execution_policy
            .vision
            .as_ref()
            .ok_or(OperationError::InvalidInput)?;
        policy.validate()?;
        if references.len() > usize::from(policy.max_images) {
            return Err(OperationError::InvalidInput);
        }
        let configuration =
            self.load_inference_configuration(request.bot, request.configuration_revision)?;
        let vision: bool = self.connection.query_row(
            "SELECT verified_vision FROM inference_configurations WHERE bot=?1 AND revision=?2",
            params![
                request.bot.to_string(),
                request.configuration_revision as i64
            ],
            |r| r.get(0),
        )?;
        if !vision
            || !self.configuration_authorized(&configuration)?
            || configuration.policy.prices.billing
                != InferenceBilling::ContextTokensIncludingImagesAtAcknowledgedCeilings
        {
            return Err(OperationError::InferenceUnverified);
        }
        let mut images = Vec::new();
        let mut pixels = 0;
        for reference in references {
            let task = self.load_task(reference.task)?;
            if task.command.owner != self.principal()?
                || task.command.bot != request.bot
                || !policy.allowed_slots.contains(&reference.slot)
            {
                return Err(OperationError::Unauthorized);
            }
            let output = task
                .outputs
                .iter()
                .find(|o| o.declaration.slot == reference.slot)
                .ok_or(OperationError::InvalidInput)?;
            let ArtifactState::Available { id, content } = &output.state else {
                return Err(OperationError::InvalidInput);
            };
            if *id != reference.artifact
                || content.sha256 != reference.sha256
                || content.length > policy.max_image_bytes
                || output.declaration.media_type != MediaType::Png
            {
                return Err(OperationError::InvalidInput);
            }
            let present: bool = self.connection.query_row("SELECT EXISTS(SELECT 1 FROM artifacts WHERE id=?1 AND task=?2 AND state='available')",
                params![id.to_string(), task.id.to_string()], |r| r.get(0))?;
            if !present {
                return Err(OperationError::InvalidInput);
            }
            let (_, width, height) =
                crate::manager::vision::image_bytes(&self.directory, reference, content.length)?;
            pixels += u64::from(width) * u64::from(height);
            if pixels > policy.max_total_pixels {
                return Err(OperationError::InvalidInput);
            }
            images.push(AuthorizedImage {
                reference: reference.clone(),
                length: content.length,
                width,
                height,
            });
        }
        Ok(images)
    }
    pub(super) fn tool_images(
        &self,
        request: &SubmitRun,
        task: TaskId,
    ) -> Result<Vec<ImageReference>, OperationError> {
        let Some(policy) = &request.execution_policy.vision else {
            return Ok(Vec::new());
        };
        Ok(self
            .load_task(task)?
            .outputs
            .iter()
            .filter_map(|o| match &o.state {
                ArtifactState::Available { id, content }
                    if o.declaration.media_type == MediaType::Png
                        && policy.allowed_slots.contains(&o.declaration.slot) =>
                {
                    Some(ImageReference {
                        task,
                        artifact: *id,
                        slot: o.declaration.slot.clone(),
                        sha256: content.sha256.clone(),
                    })
                }
                _ => None,
            })
            .collect())
    }
}
