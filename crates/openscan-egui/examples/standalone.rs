//! A complete eframe/wgpu host with caller-selected, persistent storage.
mod support;

struct App {
    scanner: openscan_egui::OpenScanUi,
    closing: bool,
}
impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        if ui.input(|input| input.viewport().close_requested()) && self.scanner.is_busy() {
            self.closing = true;
            self.scanner.cancel();
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }
        ui.add_enabled_ui(!self.closing, |ui| self.scanner.show(ui));
        if self.closing && !self.scanner.is_busy() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}
fn main() -> eframe::Result {
    let paths = match support::Paths::parse(std::env::args_os().skip(1)) {
        Ok(Some(paths)) => paths,
        Ok(None) => {
            println!(
                "OpenScan Rust\nUsage: standalone [--storage DIRECTORY] [--models DIRECTORY]\nStorage defaults to this user's application-data directory. Models default to STORAGE/models or OPENSCAN_MODELS."
            );
            return Ok(());
        }
        Err(error) => {
            eprintln!("{error}\nUse --help for usage.");
            std::process::exit(2);
        }
    };
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 780.0])
            .with_min_inner_size([400.0, 300.0])
            .with_maximized(false),
        ..Default::default()
    };
    eframe::run_native(
        "Scan — OpenScan (Rust)",
        options,
        Box::new(move |cc| {
            cc.egui_ctx.set_theme(egui::ThemePreference::Light);
            Ok(Box::new(App {
                scanner: openscan_egui::OpenScanUi::new(paths.storage, paths.models).standalone(),
                closing: false,
            }))
        }),
    )
}
