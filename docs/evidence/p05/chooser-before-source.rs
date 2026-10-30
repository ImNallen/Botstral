async fn export_artifact(
    window: WebviewWindow,
    app: tauri::AppHandle,
    service: tauri::State<'_, RemoteService>,
    task: botstral_link::TaskId,
    artifact: botstral_link::ArtifactId,
) -> Result<bool, String> {
    authorize(&window).map_err(|_| "unauthorized")?;
    let generation = service.inspect().generation;
    let verified = service.artifact(task, artifact).await?;
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_file_name(&verified.name)
        .save_file(move |path| {
            let _ = tx.send(path);
        });
    let Some(path) = rx.await.map_err(|_| "dialog")? else {
        return Ok(false);
    };
    if generation != service.inspect().generation {
        return Err("stale".into());
    }
    crate::remote::save_artifact(&path.into_path().map_err(|_| "invalid_input")?, verified)?;
    Ok(true)
}
