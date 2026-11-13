//! Plan §5.0 teachability, for every arm: how many times the player asked
//! for a shelter before the agent first spent a night in one that passes
//! the test, and whether it keeps doing so after the teacher stops showing
//! the recipe. Read from the host's dusk calls and shelter test, so every
//! arm is measured the same way (my reading of "how many hints it takes to
//! learn a new goal from the player, and whether it keeps the goal after
//! the teacher fades").

/// One night: from dusk to the end of that day.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Night {
    pub day: u64,
    /// The player asked for a shelter that night (the dusk call).
    pub asked: bool,
    /// The agent stood in a passing shelter at some point that night.
    pub sheltered: bool,
}

/// The nights of one run, in order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Nights(pub Vec<Night>);

impl Nights {
    /// One tick on `day`: whether it is dusk or later, whether the player
    /// asked for a shelter, and whether the shelter test passes.
    pub fn see(&mut self, day: u64, dusk: bool, asked: bool, sheltered: bool) {
        if !dusk {
            return;
        }
        if self.0.last().is_none_or(|n| n.day != day) {
            self.0.push(Night {
                day,
                ..Night::default()
            });
        }
        if let Some(n) = self.0.last_mut() {
            n.asked |= asked;
            n.sheltered |= sheltered;
        }
    }

    /// Calls heard up to and including the first sheltered night, or
    /// `None` if the agent never spent a night in a passing shelter.
    pub fn hints(&self) -> Option<u32> {
        let first = self.0.iter().position(|n| n.sheltered)?;
        Some(self.0[..=first].iter().filter(|n| n.asked).count() as u32)
    }

    /// Of the nights on day `from` or later (after the teacher stopped),
    /// the share spent in a passing shelter; NaN if there were none.
    pub fn kept(&self, from: u64) -> f32 {
        let after: Vec<&Night> = self.0.iter().filter(|n| n.day >= from).collect();
        let kept = after.iter().filter(|n| n.sheltered).count();
        kept as f32 / after.len() as f32
    }
}

/// The tick the teacher stops showing, for a teacher that shows for the
/// first `days` days (plan §5.0 "after the teacher fades").
pub fn teach_until(days: Option<u64>, day_ticks: u64) -> Option<u64> {
    days.map(|d| d * day_ticks)
}

/// Whether the teacher still shows at `tick`.
pub fn teaching(until: Option<u64>, tick: u64) -> bool {
    until.is_none_or(|u| tick < u)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nights_record_calls_and_shelter_from_dusk_on() {
        let mut n = Nights::default();
        n.see(0, false, true, true);
        assert_eq!(n, Nights::default(), "daytime is not a night");
        n.see(0, true, true, false);
        n.see(0, true, false, false);
        n.see(1, true, false, true);
        n.see(1, true, false, false);
        let want = [
            Night {
                day: 0,
                asked: true,
                sheltered: false,
            },
            Night {
                day: 1,
                asked: false,
                sheltered: true,
            },
        ];
        assert_eq!(n.0, want);
    }

    #[test]
    fn hints_count_calls_up_to_the_first_sheltered_night() {
        let night = |day, asked, sheltered| Night {
            day,
            asked,
            sheltered,
        };
        let n = Nights(vec![
            night(0, true, false),
            night(1, false, false),
            night(2, true, true),
            night(3, true, true),
        ]);
        assert_eq!(n.hints(), Some(2));
        assert_eq!(Nights(vec![night(0, false, true)]).hints(), Some(0));
        assert_eq!(Nights(vec![night(0, true, false)]).hints(), None);
        // Kept: of the nights from day 1 on, two of three.
        assert_eq!(n.kept(1), 2.0 / 3.0);
        assert_eq!(n.kept(3), 1.0);
        assert!(n.kept(4).is_nan(), "no night after the teacher stopped");
        assert_eq!(n.kept(0), 0.5);
    }

    #[test]
    fn the_teacher_shows_until_its_last_day_ends() {
        assert_eq!(teach_until(Some(2), 100), Some(200));
        assert_eq!(teach_until(None, 100), None);
        assert!(teaching(None, u64::MAX));
        assert!(teaching(Some(200), 199));
        assert!(!teaching(Some(200), 200));
    }
}
