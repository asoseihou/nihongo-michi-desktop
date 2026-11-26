//! Anki-style SM-2 scheduler (port of `SRS.schedule` in `03-srs.js`).
//!
//! card = `[due(ms), ivl(days), ease(‰), reps, lapses, state, step]`,
//! state: 0 new, 1 learning, 2 review, 3 relearning.

use crate::time::{add_days_start, day_start, fmt_ivl, DAY_MS};
use serde::{Deserialize, Serialize};

pub const STEPS: [f64; 2] = [1.0, 10.0];
pub const RELEARN: [f64; 1] = [10.0];
const MIN_MS: f64 = 60_000.0;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Card(pub [f64; 7]);

impl Card {
    /// Keeps a card's numbers sane (cards also arrive from backups and other
    /// PCs). False when it cannot be used (not finite).
    pub fn sanitize(&mut self) -> bool {
        if !self.0.iter().all(|x| x.is_finite()) {
            return false;
        }
        let a = &mut self.0;
        a[0] = a[0].clamp(0.0, 7.3e12); // due: up to year 2200
        a[1] = a[1].clamp(0.0, 36_500.0);
        a[2] = a[2].clamp(1300.0, 5000.0);
        a[3] = a[3].clamp(0.0, 1e6);
        a[4] = a[4].clamp(0.0, 1e6);
        a[5] = a[5].round().clamp(0.0, 3.0);
        a[6] = a[6].round().clamp(0.0, 10.0);
        true
    }

    pub fn due(&self) -> f64 {
        self.0[0]
    }
    pub fn ivl(&self) -> f64 {
        self.0[1]
    }
    pub fn ease(&self) -> f64 {
        self.0[2]
    }
    pub fn reps(&self) -> f64 {
        self.0[3]
    }
    pub fn lapses(&self) -> f64 {
        self.0[4]
    }
    pub fn state(&self) -> u8 {
        self.0[5] as u8
    }
    pub fn step(&self) -> f64 {
        self.0[6]
    }
    /// 0 new, 1 learning, 2 young, 3 mature (`SRS.status`).
    pub fn status(c: Option<&Card>) -> u8 {
        match c {
            None => 0,
            Some(c) if c.state() != 2 => 1,
            Some(c) => {
                if c.ivl() >= 21.0 {
                    3
                } else {
                    2
                }
            }
        }
    }
}

/// Grade 1 = Again, 2 = Hard, 3 = Good, 4 = Easy. `rnd` in [0,1) feeds the
/// interval fuzz (JS used `Math.random()` inline).
pub fn schedule(c: Option<&Card>, g: u8, now: f64, rnd: f64) -> Card {
    let [mut due, mut ivl, mut ease, mut reps, mut lapses, state0, step0] =
        c.map(|c| c.0).unwrap_or([now, 0.0, 2500.0, 0.0, 0.0, 0.0, 0.0]);
    let mut state = state0 as u8;
    let mut step = step0 as usize;
    reps += 1.0;
    let days_late = if state == 2 { ((day_start(now) - day_start(due)) / DAY_MS).max(0.0) } else { 0.0 };
    let fuzz = |n: f64| if n < 3.0 { n } else { n * (0.95 + rnd * 0.1) };
    let at = |i: usize| STEPS[i.min(STEPS.len() - 1)];
    if state == 0 || state == 1 {
        state = 1;
        match g {
            1 => {
                step = 0;
                due = now + STEPS[0] * MIN_MS;
            }
            2 => {
                let m = if step == 0 { (STEPS[0] + STEPS[1]) / 2.0 } else { at(step) * 1.5 };
                due = now + m * MIN_MS;
            }
            3 => {
                step += 1;
                if step >= STEPS.len() {
                    state = 2;
                    ivl = 1.0;
                    due = add_days_start(1, now);
                } else {
                    due = now + STEPS[step] * MIN_MS;
                }
            }
            _ => {
                state = 2;
                ivl = 4.0;
                due = add_days_start(4, now);
            }
        }
    } else if state == 2 {
        if g == 1 {
            lapses += 1.0;
            ease = (ease - 200.0).max(1300.0);
            ivl = (ivl * 0.3).round().max(1.0);
            state = 3;
            step = 0;
            due = now + RELEARN[0] * MIN_MS;
        } else {
            let n = match g {
                2 => {
                    let n = (ivl + 1.0).max(ivl * 1.2);
                    ease = (ease - 150.0).max(1300.0);
                    n
                }
                3 => (ivl + 1.0).max((ivl + days_late / 2.0) * ease / 1000.0),
                _ => {
                    let n = (ivl + 1.0).max((ivl + days_late) * ease / 1000.0 * 1.3);
                    ease += 150.0;
                    n
                }
            };
            ivl = fuzz(n).min(3650.0);
            due = add_days_start(ivl.round() as i64, now);
        }
    } else {
        match g {
            1 => due = now + RELEARN[0] * MIN_MS,
            2 => due = now + RELEARN[0] * 1.5 * MIN_MS,
            _ => {
                state = 2;
                due = add_days_start(ivl.round().max(1.0) as i64, now);
            }
        }
    }
    Card([due, (ivl * 100.0).round() / 100.0, ease, reps, lapses, state as f64, step as f64])
}

/// Label shown on the grade buttons (`previewLabel`).
pub fn preview_label(c: Option<&Card>, g: u8, now: f64) -> String {
    let n = schedule(c, g, now, 0.5);
    fmt_ivl((n.due() - now) / DAY_MS)
}

/// "Já sei" / Anki import (`markKnown`): a review card with ~`ivl_days`.
pub fn mark_known(ivl_days: f64, now: f64, rnd_ivl: f64, rnd_due: f64) -> Card {
    let ivl = ivl_days * (0.8 + rnd_ivl * 0.4);
    let first = 1 + (rnd_due * ivl.round()).floor() as i64;
    Card([add_days_start(first, now), (ivl * 100.0).round() / 100.0, 2500.0, 1.0, 0.0, 2.0, 0.0])
}

/// Bucket of a card id inside the artifact's `srs0..srs7` documents.
pub fn chunk_of(id: &str) -> String {
    let mut h: u32 = 0;
    for u in id.encode_utf16() {
        h = h.wrapping_mul(31).wrapping_add(u as u32);
    }
    format!("srs{}", h % 8)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::now_ms;

    #[test]
    fn new_card_good_twice_graduates() {
        let now = now_ms();
        let a = schedule(None, 3, now, 0.5);
        assert_eq!(a.state(), 1);
        assert_eq!(a.due(), now + 10.0 * MIN_MS);
        let b = schedule(Some(&a), 3, now, 0.5);
        assert_eq!(b.state(), 2);
        assert_eq!(b.ivl(), 1.0);
    }

    #[test]
    fn easy_new_goes_to_four_days_and_lapse_relearns() {
        let now = now_ms();
        let a = schedule(None, 4, now, 0.5);
        assert_eq!((a.state(), a.ivl()), (2, 4.0));
        let b = schedule(Some(&a), 1, a.due(), 0.5);
        assert_eq!(b.state(), 3);
        assert_eq!(b.lapses(), 1.0);
        assert_eq!(b.ease(), 2300.0);
    }

    #[test]
    fn chunk_matches_js_hash() {
        // JS: let h=0; for c of id: h=(h*31+code)>>>0; 'srs'+(h%8)
        assert_eq!(chunk_of("k:日"), {
            let mut h: u32 = 0;
            for u in "k:日".encode_utf16() {
                h = h.wrapping_mul(31).wrapping_add(u as u32);
            }
            format!("srs{}", h % 8)
        });
    }
}
