//! How well other ships are tracked, measured from every position report: how far
//! the local prediction had drifted from the reported truth, and how far the ship
//! was drawn from it. Logged as a one-line summary every ten seconds online, so a
//! session with real players gives numbers instead of impressions.

use oss_sim::RemoteCorrection;

/// Ticks between summaries.
const WINDOW_TICKS: u32 = 1000;

#[derive(Default)]
pub(super) struct Tracking {
    coasting: Vec<f32>,
    accelerating: Vec<f32>,
    shown: Vec<f32>,
    snaps: u32,
    window_start: Option<u32>,
}

impl Tracking {
    pub(super) fn record(&mut self, c: &RemoteCorrection) {
        if c.accelerated {
            self.accelerating.push(c.predicted_px);
        } else {
            self.coasting.push(c.predicted_px);
        }
        self.shown.push(c.shown_px);
        self.snaps += u32::from(c.snapped);
    }

    /// The summary for the window ending at `tick`, if one is due and anything was
    /// measured; starts the next window.
    pub(super) fn summary(&mut self, tick: u32) -> Option<String> {
        let start = *self.window_start.get_or_insert(tick);
        if tick.wrapping_sub(start) < WINDOW_TICKS {
            return None;
        }
        self.window_start = Some(tick);
        let n = self.shown.len();
        if n == 0 {
            return None;
        }
        let line = format!(
            "tracking: {n} reports, {snaps} jumps | prediction error px (median/p95/max) \
             coasting {} accelerating {} | drawn-vs-true px {}",
            stats(&mut self.coasting),
            stats(&mut self.accelerating),
            stats(&mut self.shown),
            snaps = self.snaps,
        );
        self.coasting.clear();
        self.accelerating.clear();
        self.shown.clear();
        self.snaps = 0;
        Some(line)
    }
}

fn stats(v: &mut [f32]) -> String {
    if v.is_empty() {
        return "-".into();
    }
    v.sort_by(f32::total_cmp);
    let at = |q: f32| v[((v.len() - 1) as f32 * q).round() as usize];
    format!(
        "{:.1}/{:.1}/{:.1} (n={})",
        at(0.5),
        at(0.95),
        v[v.len() - 1],
        v.len()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(predicted_px: f32, accelerated: bool) -> RemoteCorrection {
        RemoteCorrection {
            predicted_px,
            shown_px: predicted_px,
            snapped: false,
            accelerated,
        }
    }

    #[test]
    fn summarises_every_ten_seconds() {
        let mut t = Tracking::default();
        assert_eq!(t.summary(0), None);
        for px in [1.0, 2.0, 3.0] {
            t.record(&c(px, false));
        }
        t.record(&c(9.0, true));
        assert_eq!(t.summary(500), None);
        let line = t.summary(1000).unwrap();
        assert!(line.contains("4 reports"), "{line}");
        assert!(line.contains("coasting 2.0/3.0/3.0 (n=3)"), "{line}");
        assert!(line.contains("accelerating 9.0/9.0/9.0 (n=1)"), "{line}");
        // The next window starts empty.
        assert_eq!(t.summary(2000), None);
    }
}
