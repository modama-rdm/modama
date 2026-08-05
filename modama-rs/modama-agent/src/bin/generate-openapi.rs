fn main() -> Result<(), Box<dyn std::error::Error>> {
    let document = modama_agent::http_api::openapi_document()?;
    println!("{}", serde_json::to_string_pretty(&document)?);
    Ok(())
}
