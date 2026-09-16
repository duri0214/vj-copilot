mod domain;
mod infra;
mod launch;
mod ui;

use eframe::egui;

use crate::{
    infra::media::load_media_directory,
    launch::{parse_args, usage},
    ui::{AppConfig, VjApp},
};

fn main() {
    let options = parse_args();
    if options.show_help {
        println!("{}", usage());
        return;
    }

    let media_report = load_media_directory(options.media_dir.as_deref());
    let app_config = AppConfig {
        media_dir: options.media_dir,
        demo: options.demo,
        notices: options.notices,
        media_report,
    };
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([760.0, 720.0])
            .with_min_inner_size([700.0, 620.0]),
        ..Default::default()
    };

    if let Err(error) = eframe::run_native(
        "VJ Copilot",
        native_options,
        Box::new(move |_creation_context| Ok(Box::new(VjApp::new(app_config)))),
    ) {
        eprintln!("アプリを起動できませんでした: {error}");
    }
}
