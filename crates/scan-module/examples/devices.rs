fn main() -> Result<(), Box<dyn std::error::Error>> {
    let backend = scan_module::native_backend();
    println!("{}", backend.capabilities().name);
    for device in backend.devices()? {
        println!("{}\t{}", device.id, device.name);
        match backend.device_capabilities(&device.id) {
            Ok(capabilities) => {
                for source in capabilities.sources {
                    println!("  {:?}: {:?} DPI", source.source, source.resolutions);
                }
                println!("  {}", capabilities.note);
            }
            Err(error) => eprintln!("  Capabilities: {error}"),
        }
    }
    Ok(())
}
