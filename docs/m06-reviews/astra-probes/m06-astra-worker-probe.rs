#[test]
#[ignore = "Requires the isolated state from m06-astra-configure.py or m06-astra-recovery.py"]
fn astra_connector_configuration_probe() {
    let path = std::path::PathBuf::from(std::env::var("M06_ASTRA_STATE").unwrap());
    let result = Configuration::read(&path);
    eprintln!("Production worker configuration accepted: {}", matches!(result, Ok(Some(_))));
    if let Err(error) = result {
        eprintln!("Production worker configuration error: {error}");
    }
}
