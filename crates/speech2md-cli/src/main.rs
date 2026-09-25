mod logging;

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    logging::init()?;
    tracing::debug!("logging initialized");
    Ok(())
}
