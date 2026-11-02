//! Inspect a HEIC gain map without a Swift host.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os().nth(1).ok_or("expected HEIC path")?;
    let data = std::fs::read(path)?;
    let map = momentstays_core::heic_gain::read(&data)?;
    let value = map.map(|m| {
        serde_json::json!({
            "versions": m.versions, "headrooms": m.headrooms,
            "image": m.image.map(|i| serde_json::json!({"width":i.width,"height":i.height,
                "stride":i.stride,"sha256":momentstays_core::sha256_hex(&i.pixels)}))
        })
    });
    println!("{}", serde_json::to_string(&value)?);
    Ok(())
}
