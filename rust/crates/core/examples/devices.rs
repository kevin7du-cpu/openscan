use openscan_core::{Cancellation, backend};
fn main() {
    let (devices, warnings) = backend::devices(&Cancellation::default());
    println!(
        "{}",
        serde_json::json!({"devices":devices,"warnings":warnings})
    );
}
