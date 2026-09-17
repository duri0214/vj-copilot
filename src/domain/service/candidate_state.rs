use std::collections::BTreeSet;

use crate::domain::valueobject::ClipId;

pub const PREVIEW_SLOT_COUNT: usize = 4;

#[derive(Debug)]
pub struct CandidateState {
    slots: [Option<ClipId>; PREVIEW_SLOT_COUNT],
    held: bool,
    selected_slot: Option<usize>,
}

impl Default for CandidateState {
    fn default() -> Self {
        Self {
            slots: std::array::from_fn(|_| None),
            held: false,
            selected_slot: None,
        }
    }
}

impl CandidateState {
    pub fn slots(&self) -> &[Option<ClipId>; PREVIEW_SLOT_COUNT] {
        &self.slots
    }

    pub fn is_held(&self) -> bool {
        self.held
    }

    pub fn selected_slot(&self) -> Option<usize> {
        self.selected_slot
    }

    pub fn update_ranked(&mut self, ranked: &[ClipId]) -> bool {
        if self.held {
            return false;
        }

        let ranked: Vec<ClipId> = ranked.iter().take(PREVIEW_SLOT_COUNT).cloned().collect();
        let mut unmatched: BTreeSet<ClipId> = ranked.iter().cloned().collect();
        let previous_slots = self.slots.clone();

        for slot in &mut self.slots {
            let keep_current_slot = slot
                .as_ref()
                .is_some_and(|clip_id| unmatched.remove(clip_id));

            if !keep_current_slot {
                *slot = None;
            }
        }

        let mut replacements = ranked
            .into_iter()
            .filter(|clip_id| unmatched.remove(clip_id));
        for slot in &mut self.slots {
            if slot.is_none() {
                *slot = replacements.next();
            }
        }

        previous_slots != self.slots
    }

    pub fn select(&mut self, slot: usize) -> bool {
        if self.slots.get(slot).is_some_and(Option::is_some) {
            self.held = true;
            self.selected_slot = Some(slot);
            true
        } else {
            false
        }
    }

    pub fn toggle_hold(&mut self) {
        if self.held {
            self.release();
        } else {
            self.held = true;
            self.selected_slot = None;
        }
    }

    pub fn release(&mut self) {
        self.held = false;
        self.selected_slot = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(value: &str) -> ClipId {
        ClipId::new(value).unwrap()
    }

    #[test]
    fn retains_candidates_in_their_existing_slots() {
        let mut state = CandidateState::default();
        state.update_ranked(&[id("a.mp4"), id("b.mp4"), id("c.mp4"), id("d.mp4")]);

        state.update_ranked(&[id("b.mp4"), id("c.mp4"), id("e.mp4"), id("f.mp4")]);

        assert_eq!(state.slots()[1].as_ref(), Some(&id("b.mp4")));
        assert_eq!(state.slots()[2].as_ref(), Some(&id("c.mp4")));
        assert_eq!(state.slots()[0].as_ref(), Some(&id("e.mp4")));
        assert_eq!(state.slots()[3].as_ref(), Some(&id("f.mp4")));
    }

    #[test]
    fn selection_holds_candidates_until_release() {
        let mut state = CandidateState::default();
        state.update_ranked(&[id("a.mp4"), id("b.mp4")]);

        assert!(state.select(0));
        assert!(state.is_held());
        assert_eq!(state.selected_slot(), Some(0));
        assert!(!state.update_ranked(&[id("c.mp4"), id("d.mp4")]));
        assert_eq!(state.slots()[0].as_ref(), Some(&id("a.mp4")));

        state.release();
        state.update_ranked(&[id("c.mp4"), id("d.mp4")]);

        assert!(!state.is_held());
        assert_eq!(state.selected_slot(), None);
        assert_eq!(state.slots()[0].as_ref(), Some(&id("c.mp4")));
    }
}
