use super::*;
use crate::manager::provider::reflects_secret;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, io::Read, os::unix::fs::OpenOptionsExt};

pub(in crate::manager) fn collect(permit: CollectPermit) -> Result<String, ()> {
    let mut artifacts = Vec::new();
    let mut text_budget = MAX_RUN_TEXT_BYTES;
    for output in &permit.task.outputs {
        let mut artifact = json!(output);
        match &output.state {
            ArtifactState::Unavailable { .. } => {}
            ArtifactState::Available { id, content } => {
                if content.length > output.declaration.max_bytes
                    || content.length > MAX_ARTIFACT_BYTES
                {
                    return Err(());
                }
                let mut file = fs::OpenOptions::new()
                    .read(true)
                    .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
                    .open(
                        permit
                            .directory
                            .join("artifacts")
                            .join(format!("{id}.blob")),
                    )
                    .map_err(|_| ())?;
                let metadata = file.metadata().map_err(|_| ())?;
                if !metadata.is_file() || metadata.len() != content.length {
                    return Err(());
                }
                let mut hash = Sha256::new();
                let mut total = 0;
                let mut bytes = Vec::new();
                let mut buffer = [0; 16384];
                loop {
                    let n = file.read(&mut buffer).map_err(|_| ())?;
                    if n == 0 {
                        break;
                    }
                    total += n as u64;
                    if total > content.length {
                        return Err(());
                    }
                    hash.update(&buffer[..n]);
                    if output.declaration.media_type == MediaType::TextPlain
                        && content.length <= text_budget as u64
                    {
                        bytes.extend_from_slice(&buffer[..n]);
                    }
                }
                if total != content.length
                    || format!("{:x}", hash.finalize()) != content.sha256.as_str()
                {
                    return Err(());
                }
                if output.declaration.media_type == MediaType::TextPlain
                    && content.length <= text_budget as u64
                {
                    let text = String::from_utf8(bytes).map_err(|_| ())?;
                    text_budget -= text.len();
                    artifact["untrusted_text"] = text.into();
                }
            }
            _ => return Err(()),
        }
        artifacts.push(artifact);
    }
    let process = permit.task.process.as_ref().map(|evidence| {
        let output = |stream: &CapturedOutput| {
            let bytes = (0..stream.hex.len()).step_by(2).map(|i|u8::from_str_radix(&stream.hex[i..i+2],16)).collect::<Result<Vec<_>,_>>().ok();
            json!({"text":bytes.and_then(|b|String::from_utf8(b).ok()), "complete":stream.complete, "truncated":stream.truncated})
        };
        json!({"effect":evidence.effect,"result": evidence.result.as_ref().map(|r|json!({"exit":r.exit,"stdout":output(&r.stdout),"stderr":output(&r.stderr)}))})
    });
    let value: Value = json!({"untrusted_tool_result":{"task":permit.task.id,"state":permit.task.state,"outcome":permit.task.outcome,"process":process,"artifacts":artifacts}});
    if reflects_secret(&value, permit.secret.expose_secret()) {
        return Err(());
    }
    let result = serde_json::to_string(&value).map_err(|_| ())?;
    if result.len() > MAX_RUN_RESULT_BYTES {
        return Err(());
    }
    Ok(result)
}
