use crate::*;

pub const MAX_IMAGE_BYTES: u64 = 1_048_576;
pub const MAX_IMAGE_PIXELS: u64 = 1_638_400;
pub const MAX_IMAGE_DIMENSION: u32 = 1280;
pub const MAX_IMAGES: usize = 2;
pub const MAX_MULTIMODAL_REQUEST: usize = 3 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VisionPolicy {
    pub allowed_slots: Vec<String>,
    pub max_images: u16,
    pub max_image_bytes: u64,
    pub max_total_pixels: u64,
}
impl VisionPolicy {
    pub fn validate(&self) -> Result<(), ErrorCode> {
        if self.allowed_slots.is_empty()
            || self.allowed_slots.len() > MAX_OUTPUTS
            || self.allowed_slots.iter().any(|s| !valid_slot(s))
            || !(1..=MAX_IMAGES as u16).contains(&self.max_images)
            || !(1..=MAX_IMAGE_BYTES).contains(&self.max_image_bytes)
            || !(1..=MAX_IMAGE_PIXELS * MAX_IMAGES as u64).contains(&self.max_total_pixels)
        {
            return Err(ErrorCode::InvalidMessage);
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImageReference {
    pub task: TaskId,
    pub artifact: ArtifactId,
    pub slot: String,
    pub sha256: CertificateFingerprint,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AuthorizedImage {
    pub reference: ImageReference,
    pub length: u64,
    pub width: u32,
    pub height: u32,
}
