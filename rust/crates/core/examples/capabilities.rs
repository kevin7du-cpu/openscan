use openscan_core::{Cancellation, backend};
fn main() {
    let (devices, _) = backend::devices(&Cancellation::default());
    for device in devices.into_iter().filter(|d| d.driver == "WIA") {
        let result = backend::for_device(&device.id).and_then(|b| b.capabilities(&device.id));
        match result {
            Ok(c) => println!(
                "{}",
                serde_json::json!({"name":device.name,"sources":c.sources})
            ),
            Err(e) => println!(
                "{}",
                serde_json::json!({"name":device.name,"error":e.to_string()})
            ),
        }
    }
}
