use eframe::egui::IconData;

const ICON_SIZE: usize = 32;

pub fn app_icon() -> IconData {
    let mut rgba = vec![0; ICON_SIZE * ICON_SIZE * 4];
    for pixel in rgba.as_chunks_mut::<4>().0.iter_mut() {
        pixel.copy_from_slice(&[0, 0, 0, 255]);
    }
    for y in 0..ICON_SIZE {
        for x in 0..ICON_SIZE {
            let in_e = (7..=24).contains(&y)
                && (7..=25).contains(&x)
                && ((y <= 10 && (10..=24).contains(&x))
                    || ((11..=14).contains(&y) && (8..=24).contains(&x))
                    || ((15..=18).contains(&y) && (8..=23).contains(&x))
                    || ((19..=22).contains(&y) && (9..=24).contains(&x))
                    || ((23..=24).contains(&y) && (10..=22).contains(&x)));
            if !in_e {
                continue;
            }
            let offset = (y * ICON_SIZE + x) * 4;
            rgba[offset..offset + 4].copy_from_slice(&[255, 255, 255, 255]);
        }
    }
    IconData {
        rgba,
        width: ICON_SIZE as u32,
        height: ICON_SIZE as u32,
    }
}
