use eframe::egui::IconData;

pub fn app_icon() -> IconData {
    eframe::icon_data::from_png_bytes(include_bytes!("../assets/eframe-icon.png"))
        .expect("embedded eframe icon must be valid PNG")
}
