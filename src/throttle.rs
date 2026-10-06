use std::time::Instant;

pub struct Throttle {
    limit: Option<f64>,
    start: Instant,
    sent: u64,
}
impl Throttle {
    pub fn new(limit: Option<f64>) -> Self {
        Self {
            limit,
            start: Instant::now(),
            sent: 0,
        }
    }
    pub fn allow(&mut self, now: Instant) -> bool {
        let Some(limit) = self.limit else {
            return true;
        };
        let elapsed = now.saturating_duration_since(self.start);
        // Match the supplied average-frequency gate, resetting after the decision.
        let allow = elapsed.as_secs_f64() > 0.0
            && (self.sent as f64 + 1.0) / elapsed.as_secs_f64() <= limit;
        if allow {
            self.sent += 1;
        }
        let period = (1.0 / limit).max(1.0);
        if elapsed.as_secs_f64() > period {
            self.start = now;
            self.sent = 0;
        }
        allow
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    #[test]
    fn matches_average_frequency_gate() {
        let mut t = Throttle::new(Some(10.0));
        let start = t.start;
        assert!(!t.allow(start));
        assert!(!t.allow(start + Duration::from_millis(99)));
        assert!(t.allow(start + Duration::from_millis(100)));
        assert!(!t.allow(start + Duration::from_millis(199)));
        assert!(t.allow(start + Duration::from_millis(200)));
        assert!(t.allow(start + Duration::from_millis(1001)));
        assert!(!t.allow(start + Duration::from_millis(1002)));
        assert!(t.allow(start + Duration::from_millis(1101)));
    }
    #[test]
    fn unlimited_and_low_frequency() {
        assert!(Throttle::new(None).allow(Instant::now()));
        let mut t = Throttle::new(Some(0.5));
        let start = t.start;
        assert!(!t.allow(start + Duration::from_secs(1)));
        assert!(t.allow(start + Duration::from_secs(2)));
    }
    #[test]
    fn sub_hertz_with_continuous_input() {
        let mut t = Throttle::new(Some(0.5));
        let start = t.start;
        let count = (0..=100)
            .filter(|i| t.allow(start + Duration::from_millis(i * 100)))
            .count();
        assert_eq!(count, 4);
    }
}
