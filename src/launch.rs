use std::path::PathBuf;

pub struct LaunchOptions {
    pub media_dir: Option<PathBuf>,
    pub demo: bool,
    pub notices: Vec<String>,
    pub show_help: bool,
}

pub fn parse_args() -> LaunchOptions {
    let mut media_dir = None;
    let mut demo = false;
    let mut notices = Vec::new();
    let mut show_help = false;
    let mut args = std::env::args_os().skip(1);

    while let Some(argument) = args.next() {
        match argument.to_string_lossy().as_ref() {
            "--media-dir" => match args.next() {
                Some(directory) => media_dir = Some(PathBuf::from(directory)),
                None => notices.push("--media-dir の後にフォルダを指定してください".to_owned()),
            },
            "--demo" => demo = true,
            "--help" | "-h" => show_help = true,
            unknown => notices.push(format!("未対応の引数です: {unknown}")),
        }
    }

    LaunchOptions {
        media_dir,
        demo,
        notices,
        show_help,
    }
}

pub fn usage() -> &'static str {
    "Usage: cargo run --release -- --media-dir <folder> [--demo]"
}
