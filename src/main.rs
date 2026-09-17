mod domain;
mod infra;
mod launch;
mod ui;

#[cfg(windows)]
use std::{fs, path::PathBuf};

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
            .with_inner_size([640.0, 720.0])
            .with_min_inner_size([540.0, 540.0]),
        ..Default::default()
    };

    if let Err(error) = eframe::run_native(
        "VJ Copilot",
        native_options,
        Box::new(move |creation_context| {
            install_japanese_font(&creation_context.egui_ctx);
            ui::theme::install(&creation_context.egui_ctx);
            Ok(Box::new(VjApp::new(app_config)))
        }),
    ) {
        eprintln!("アプリを起動できませんでした: {error}");
    }
}

fn install_japanese_font(context: &egui::Context) {
    let Some(font_bytes) = japanese_font_bytes() else {
        return;
    };

    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "windows-japanese".to_owned(),
        std::sync::Arc::new(egui::FontData::from_owned(font_bytes)),
    );

    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        if let Some(font_names) = fonts.families.get_mut(&family) {
            font_names.push("windows-japanese".to_owned());
        }
    }

    context.set_fonts(fonts);
}

#[cfg(windows)]
fn japanese_font_bytes() -> Option<Vec<u8>> {
    let windows_dir = std::env::var_os("WINDIR").map(PathBuf::from)?;
    [
        "Fonts\\NotoSansJP-VF.ttf",
        "Fonts\\YuGothR.ttc",
        "Fonts\\meiryo.ttc",
        "Fonts\\msgothic.ttc",
    ]
    .iter()
    .map(|relative_path| windows_dir.join(relative_path))
    .find_map(|path| fs::read(path).ok())
}

#[cfg(not(windows))]
fn japanese_font_bytes() -> Option<Vec<u8>> {
    None
}
