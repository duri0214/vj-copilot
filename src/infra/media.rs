use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use serde::Deserialize;

use crate::domain::valueobject::{ClipId, ClipMetadata, FeatureVector};

pub const VIDEO_WIDTH: usize = 320;
pub const VIDEO_HEIGHT: usize = 180;
pub const VIDEO_FRAMES_PER_SECOND: usize = 15;
pub const MAX_VIDEO_SECONDS: usize = 3;
pub const MAX_VIDEO_FRAMES: usize = VIDEO_FRAMES_PER_SECOND * MAX_VIDEO_SECONDS;
const MAX_CLIPS: usize = 8;
const FRAME_BYTES: usize = VIDEO_WIDTH * VIDEO_HEIGHT * 4;

#[derive(Debug)]
pub struct VideoFrame {
    pub rgba: Vec<u8>,
}

#[derive(Debug)]
pub struct MediaClip {
    pub metadata: ClipMetadata,
    pub frames: Vec<VideoFrame>,
}

#[derive(Debug, Default)]
pub struct MediaLibrary {
    clips: Vec<MediaClip>,
}

impl MediaLibrary {
    pub fn metadata(&self) -> impl Iterator<Item = &ClipMetadata> {
        self.clips.iter().map(|clip| &clip.metadata)
    }

    pub fn find(&self, id: &ClipId) -> Option<&MediaClip> {
        self.clips.iter().find(|clip| &clip.metadata.id == id)
    }

    pub fn len(&self) -> usize {
        self.clips.len()
    }
}

pub struct MediaLoadReport {
    pub library: MediaLibrary,
    pub notices: Vec<String>,
}

#[derive(Deserialize)]
struct ManifestEntry {
    file: String,
    energy: f32,
    brightness: f32,
}

pub fn load_media_directory(media_dir: Option<&Path>) -> MediaLoadReport {
    let mut report = MediaLoadReport {
        library: MediaLibrary::default(),
        notices: Vec::new(),
    };

    let Some(media_dir) = media_dir else {
        report.notices.push(
            "メディアフォルダが未指定です。--media-dir <folder> を指定してください".to_owned(),
        );
        return report;
    };

    if !media_dir.is_dir() {
        report.notices.push(format!(
            "メディアフォルダが見つかりません: {}",
            media_dir.display()
        ));
        return report;
    }

    let mp4_files = match collect_mp4_files(media_dir) {
        Ok(files) => files,
        Err(error) => {
            report.notices.push(format!(
                "メディアフォルダを読めません: {} ({error})",
                media_dir.display()
            ));
            return report;
        }
    };

    let manifest_path = media_dir.join("clips.json");
    let manifest_text = match fs::read_to_string(&manifest_path) {
        Ok(text) => text,
        Err(error) => {
            report.notices.push(format!(
                "clips.json を読めません: {} ({error})",
                manifest_path.display()
            ));
            return report;
        }
    };
    let manifest: Vec<ManifestEntry> = match serde_json::from_str(&manifest_text) {
        Ok(manifest) => manifest,
        Err(error) => {
            report
                .notices
                .push(format!("clips.json の形式が不正です: {error}"));
            return report;
        }
    };

    let mut referenced_files = BTreeSet::new();
    let mut candidates = Vec::new();
    for entry in manifest {
        if !is_direct_file_name(&entry.file) {
            report.notices.push(format!(
                "metadata を除外しました（file はフォルダ直下のファイル名で指定してください）: {}",
                entry.file
            ));
            continue;
        }
        if !referenced_files.insert(entry.file.clone()) {
            report.notices.push(format!(
                "metadata を除外しました（file が重複しています）: {}",
                entry.file
            ));
            continue;
        }

        let Some(path) = mp4_files.get(&entry.file) else {
            report.notices.push(format!(
                "metadata を除外しました（MP4 が見つかりません）: {}",
                entry.file
            ));
            continue;
        };
        let features = match FeatureVector::new(entry.energy, entry.brightness) {
            Ok(features) => features,
            Err(error) => {
                report.notices.push(format!(
                    "metadata を除外しました（{error}）: {}",
                    entry.file
                ));
                continue;
            }
        };
        let id = match ClipId::new(entry.file.clone()) {
            Ok(id) => id,
            Err(error) => {
                report.notices.push(format!(
                    "metadata を除外しました（{error}）: {}",
                    entry.file
                ));
                continue;
            }
        };

        candidates.push((ClipMetadata::new(id, features), path.clone()));
    }

    for file_name in mp4_files.keys() {
        if !referenced_files.contains(file_name) {
            report.notices.push(format!(
                "MP4 を除外しました（clips.json に metadata がありません）: {file_name}"
            ));
        }
    }

    candidates.sort_by(|(left, _), (right, _)| left.id.cmp(&right.id));
    if candidates.len() > MAX_CLIPS {
        report.notices.push(format!(
            "最大 {MAX_CLIPS} 本のため、{} 本の metadata を除外しました",
            candidates.len() - MAX_CLIPS
        ));
        candidates.truncate(MAX_CLIPS);
    }

    if candidates.is_empty() {
        report
            .notices
            .push("有効な素材がありません。Nothing を表示します".to_owned());
        return report;
    }

    if let Err(error) = check_ffmpeg() {
        report.notices.push(error);
        return report;
    }

    for (metadata, path) in candidates {
        match decode_preview(&path) {
            Ok(frames) => report.library.clips.push(MediaClip { metadata, frames }),
            Err(error) => report.notices.push(format!(
                "MP4 を除外しました（{}）: {}",
                error,
                path.display()
            )),
        }
    }

    if report.library.len() < 4 {
        report.notices.push(format!(
            "有効素材は {} 本です。不足分は Nothing で表示します",
            report.library.len()
        ));
    }

    report
}

fn collect_mp4_files(media_dir: &Path) -> Result<BTreeMap<String, PathBuf>, std::io::Error> {
    let mut files = BTreeMap::new();

    for entry in fs::read_dir(media_dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }

        let path = entry.path();
        let is_mp4 = path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("mp4"));
        if is_mp4 {
            files.insert(entry.file_name().to_string_lossy().into_owned(), path);
        }
    }

    Ok(files)
}

fn is_direct_file_name(file: &str) -> bool {
    Path::new(file)
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name == file)
}

fn check_ffmpeg() -> Result<(), String> {
    let output = Command::new("ffmpeg")
        .arg("-version")
        .output()
        .map_err(|error| format!("FFmpeg を起動できません。PATH を確認してください: {error}"))?;

    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "FFmpeg が正常終了しませんでした: {}",
            ffmpeg_message(&output.stderr)
        ))
    }
}

fn decode_preview(path: &Path) -> Result<Vec<VideoFrame>, String> {
    let output = Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-nostdin", "-i"])
        .arg(path)
        .args([
            "-t",
            "3",
            "-an",
            "-vf",
            "scale=320:180,fps=15,format=rgba",
            "-frames:v",
            "45",
            "-f",
            "rawvideo",
            "-",
        ])
        .output()
        .map_err(|error| format!("FFmpeg を起動できません: {error}"))?;

    if !output.status.success() {
        return Err(format!(
            "FFmpeg の変換に失敗しました: {}",
            ffmpeg_message(&output.stderr)
        ));
    }

    let frame_count = (output.stdout.len() / FRAME_BYTES).min(MAX_VIDEO_FRAMES);
    if frame_count == 0 {
        return Err("FFmpeg から RGBA フレームを取得できませんでした".to_owned());
    }

    Ok(output.stdout[..frame_count * FRAME_BYTES]
        .chunks(FRAME_BYTES)
        .map(|rgba| VideoFrame {
            rgba: rgba.to_vec(),
        })
        .collect())
}

fn ffmpeg_message(stderr: &[u8]) -> String {
    let message = String::from_utf8_lossy(stderr);
    let compact = message.split_whitespace().collect::<Vec<_>>().join(" ");

    if compact.is_empty() {
        "詳細はありません".to_owned()
    } else {
        compact.chars().take(240).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_a_direct_filename_in_manifest_entries() {
        assert!(is_direct_file_name("clip.mp4"));
        assert!(!is_direct_file_name("nested/clip.mp4"));
        assert!(!is_direct_file_name(""));
    }
}
