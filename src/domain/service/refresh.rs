use std::time::Duration;

const CANDIDATE_REFRESH_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Debug, Default)]
pub struct CandidateRefresh {
    last_refresh: Option<Duration>,
}

impl CandidateRefresh {
    pub fn due_at(&mut self, elapsed: Duration) -> bool {
        let due = match self.last_refresh {
            Some(last_refresh) => {
                elapsed.saturating_sub(last_refresh) >= CANDIDATE_REFRESH_INTERVAL
            }
            None => true,
        };

        if due {
            self.last_refresh = Some(elapsed);
        }

        due
    }

    pub fn reset(&mut self) {
        self.last_refresh = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refreshes_immediately_then_every_two_seconds_without_waiting() {
        let mut refresh = CandidateRefresh::default();

        assert!(refresh.due_at(Duration::ZERO));
        assert!(!refresh.due_at(Duration::from_millis(1_999)));
        assert!(refresh.due_at(Duration::from_millis(2_000)));
    }
}
