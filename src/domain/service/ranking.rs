use crate::domain::valueobject::{ClipId, ClipMetadata, FeatureVector};

use super::PREVIEW_SLOT_COUNT;

pub fn rank_clips<'a>(
    target: FeatureVector,
    clips: impl IntoIterator<Item = &'a ClipMetadata>,
) -> Vec<ClipId> {
    let mut ranked: Vec<(f32, ClipId)> = clips
        .into_iter()
        .map(|clip| (squared_distance(target, clip.features), clip.id.clone()))
        .collect();

    ranked.sort_by(|(left_distance, left_id), (right_distance, right_id)| {
        left_distance
            .total_cmp(right_distance)
            .then_with(|| left_id.cmp(right_id))
    });

    ranked
        .into_iter()
        .take(PREVIEW_SLOT_COUNT)
        .map(|(_, id)| id)
        .collect()
}

fn squared_distance(left: FeatureVector, right: FeatureVector) -> f32 {
    let energy_difference = left.energy() - right.energy();
    let brightness_difference = left.brightness() - right.brightness();

    energy_difference * energy_difference + brightness_difference * brightness_difference
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip(file: &str, energy: f32, brightness: f32) -> ClipMetadata {
        ClipMetadata::new(
            ClipId::new(file).unwrap(),
            FeatureVector::new(energy, brightness).unwrap(),
        )
    }

    #[test]
    fn ranks_by_squared_distance_then_filename() {
        let clips = vec![
            clip("z.mp4", 0.4, 0.4),
            clip("b.mp4", 0.6, 0.5),
            clip("a.mp4", 0.5, 0.6),
            clip("near.mp4", 0.51, 0.51),
        ];

        let result = rank_clips(FeatureVector::new(0.5, 0.5).unwrap(), &clips);
        let names: Vec<&str> = result.iter().map(ClipId::as_str).collect();

        assert_eq!(names, vec!["near.mp4", "a.mp4", "b.mp4", "z.mp4"]);
    }

    #[test]
    fn returns_only_available_clips_when_fewer_than_four_exist() {
        let clips = vec![clip("one.mp4", 0.1, 0.1), clip("two.mp4", 0.2, 0.2)];

        let result = rank_clips(FeatureVector::new(0.2, 0.2).unwrap(), &clips);

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].as_str(), "two.mp4");
    }
}
