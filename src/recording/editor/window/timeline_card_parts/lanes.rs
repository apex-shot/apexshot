/// The secondary lanes below the video lane, in the order they are stacked.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum TimelineLane {
    #[default]
    Zoom,
    Sound,
}

impl TimelineLane {
    /// Every lane, in stacking order.
    pub const ALL: [Self; 2] = [Self::Zoom, Self::Sound];

    /// The lane's slot in the stack, which is also its slot in the per-lane
    /// arrays.
    pub fn index(self) -> usize {
        match self {
            Self::Zoom => 0,
            Self::Sound => 1,
        }
    }
}

/// Height of a lane drawn at full height, and of one resting as a thin strip.
pub const LANE_OPEN_HEIGHT: f64 = 42.0;
pub const LANE_THIN_HEIGHT: f64 = 18.0;

/// Which secondary lane is open (drawn at full height) and how far each lane
/// is through its ease. Progress is `[zoom, sound]`, 0 resting .. 1 open, and
/// always sums to one expanded lane, so the stack keeps one height while the
/// lanes trade it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TimelineLanes {
    open: TimelineLane,
    progress: [f64; 2],
    /// The selection last seen, so a new selection brings its lane forward.
    seen_selection: Option<usize>,
}

impl Default for TimelineLanes {
    fn default() -> Self {
        Self {
            open: TimelineLane::default(),
            progress: [1.0, 0.0],
            seen_selection: None,
        }
    }
}

impl TimelineLanes {
    /// Whether `lane` is the one drawn at full height.
    pub fn is_open(&self, lane: TimelineLane) -> bool {
        self.open == lane
    }

    /// Make `lane` the full-height one. Returns whether it changed.
    pub fn open(&mut self, lane: TimelineLane) -> bool {
        let changed = self.open != lane;
        self.open = lane;
        changed
    }

    /// Fall back to the Zoom lane when the open one is no longer present.
    /// Returns whether the open lane changed.
    pub fn retain_present(&mut self, present: [bool; 2]) -> bool {
        if present[self.open.index()] {
            return false;
        }
        self.open(TimelineLane::Zoom)
    }

    /// A newly selected zoom opens its lane, so a selection made from the
    /// clip menu or by adding a clip is always in view.
    pub fn follow_selection(&mut self, zoom: Option<usize>) {
        if self.seen_selection == zoom {
            return;
        }
        self.seen_selection = zoom;
        if zoom.is_some() {
            self.open(TimelineLane::Zoom);
        }
    }

    fn targets(&self) -> [f64; 2] {
        let mut targets = [0.0; 2];
        for lane in TimelineLane::ALL {
            targets[lane.index()] = f64::from(u8::from(self.is_open(lane)));
        }
        targets
    }

    /// How far each lane is drawn toward full height, 0 resting .. 1 open.
    pub fn progress(&self) -> [f64; 2] {
        self.progress
    }

    /// Whether every lane has arrived, so the ease can stop asking for frames.
    pub fn settled(&self) -> bool {
        self.progress == self.targets()
    }

    /// Ease every lane toward its target (1 for the open lane, 0 for the
    /// rest) by `dt` seconds. Returns whether anything is still moving.
    pub fn step(&mut self, dt: f64) -> bool {
        // About 180 ms to settle: an exponential ease with a 45 ms constant.
        let amount = 1.0 - (-dt.max(0.0) / 0.045).exp();
        let targets = self.targets();
        let mut next = self.progress;
        for index in 0..2 {
            next[index] += (targets[index] - next[index]) * amount;
        }
        // Snap only once every lane has arrived: snapping one early would
        // break their sum, which is what keeps the stack's height fixed.
        let arrived = (0..2).all(|index| (targets[index] - next[index]).abs() < 0.002);
        self.progress = if arrived { targets } else { next };
        !arrived
    }

    /// A lane's height for how open it is drawn, in pixels.
    fn height_exact(progress: f64) -> f64 {
        let progress = progress.clamp(0.0, 1.0);
        LANE_THIN_HEIGHT + (LANE_OPEN_HEIGHT - LANE_THIN_HEIGHT) * progress
    }

    /// Whole-pixel heights for the lanes that are present, whose sum never
    /// changes while they trade height: each lane takes the difference between
    /// the rounded running totals, so a pixel one lane loses to rounding the
    /// next one gains. Rounding each lane on its own made the total wobble by
    /// a pixel a frame, and the preview above resized with it.
    pub fn heights(&self, present: [bool; 2]) -> [i32; 2] {
        let mut heights = [0; 2];
        let shown: Vec<usize> = (0..2).filter(|&index| present[index]).collect();
        // The resting total: one lane full and the rest thin, in whole pixels.
        // Pinning the sum to it keeps a total that lands on half a pixel from
        // flipping between frames on floating-point noise.
        let resting = (Self::height_exact(1.0)
            + Self::height_exact(0.0) * shown.len().saturating_sub(1) as f64)
            .round() as i32;
        let mut total = 0.0;
        let mut placed = 0;
        for (position, &index) in shown.iter().enumerate() {
            if position + 1 == shown.len() {
                heights[index] = resting - placed;
                break;
            }
            total += Self::height_exact(self.progress[index]);
            let rounded = total.round() as i32;
            heights[index] = rounded - placed;
            placed = rounded;
        }
        for index in 0..2 {
            if !present[index] {
                heights[index] = Self::height_exact(self.progress[index]).round() as i32;
            }
        }
        heights
    }
}

/// The card's shared handle on the lane stack: the interactions open a lane and
/// ask which one is open, and the redraw does the easing. Cheap to clone, and
/// it never touches the edit state, so it is safe to use from inside a handler
/// that already holds that lock.
#[derive(Clone)]
pub struct LaneControls {
    lanes: Rc<RefCell<TimelineLanes>>,
}

impl LaneControls {
    /// Wrap the stack the card keeps for its whole life.
    pub fn new(lanes: Rc<RefCell<TimelineLanes>>) -> Self {
        Self { lanes }
    }

    /// Whether the lane is the one drawn at full height.
    pub fn is_open(&self, lane: TimelineLane) -> bool {
        self.lanes.borrow().is_open(lane)
    }

    /// Make `lane` the full-height one. The next redraw eases it open.
    pub fn open(&self, lane: TimelineLane) {
        self.lanes.borrow_mut().open(lane);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_lane_is_open_at_a_time() {
        let lanes = TimelineLanes::default();
        assert!(lanes.is_open(TimelineLane::Zoom));
        let open = TimelineLane::ALL
            .iter()
            .filter(|&&lane| lanes.is_open(lane))
            .count();
        assert_eq!(open, 1);
        assert_eq!(lanes.progress(), [1.0, 0.0]);
        assert!(lanes.settled());
    }

    #[test]
    fn a_new_selection_opens_its_lane_and_a_repeat_does_not() {
        let mut lanes = TimelineLanes::default();
        lanes.follow_selection(Some(2));
        assert!(lanes.is_open(TimelineLane::Zoom));

        // The same selection seen again must not drag the stack back.
        lanes.open(TimelineLane::Sound);
        lanes.follow_selection(Some(2));
        assert!(lanes.is_open(TimelineLane::Sound));

        // A different selection does.
        lanes.follow_selection(Some(1));
        assert!(lanes.is_open(TimelineLane::Zoom));

        // Clearing the selection changes nothing about which lane is open.
        lanes.open(TimelineLane::Sound);
        lanes.follow_selection(None);
        assert!(lanes.is_open(TimelineLane::Sound));
    }

    #[test]
    fn the_ease_reaches_its_targets_and_settles() {
        let mut lanes = TimelineLanes::default();
        lanes.open(TimelineLane::Sound);
        assert!(!lanes.settled());
        let mut steps = 0;
        while lanes.step(0.016) {
            steps += 1;
            assert!(steps < 200, "the ease must arrive rather than asymptote");
        }
        assert_eq!(lanes.progress(), [0.0, 1.0]);
        assert!(lanes.settled());
        assert!(!lanes.step(0.016), "a settled stack costs no frames");
    }

    #[test]
    fn the_lane_stack_keeps_one_height_while_the_lanes_trade() {
        for present in [[true, false], [true, true]] {
            let shown = present.iter().filter(|&&visible| visible).count();
            let total = LANE_OPEN_HEIGHT.round() as i32
                + LANE_THIN_HEIGHT.round() as i32 * (shown as i32 - 1);
            let mut lanes = TimelineLanes::default();
            lanes.open(TimelineLane::Sound);
            loop {
                let heights = lanes.heights(present);
                let sum: i32 = (0..2)
                    .filter(|&index| present[index])
                    .map(|index| heights[index])
                    .sum();
                assert_eq!(sum, total, "present {present:?} wobbled to {heights:?}");
                for index in 0..2 {
                    if present[index] {
                        assert!(heights[index] >= LANE_THIN_HEIGHT.round() as i32);
                        assert!(heights[index] <= LANE_OPEN_HEIGHT.round() as i32);
                    }
                }
                if !lanes.step(0.016) {
                    break;
                }
            }
        }
    }

    #[test]
    fn an_open_lane_that_goes_away_falls_back_to_zoom() {
        let mut lanes = TimelineLanes::default();
        lanes.open(TimelineLane::Sound);
        assert!(lanes.retain_present([true, false]));
        assert!(lanes.is_open(TimelineLane::Zoom));
        assert!(!lanes.retain_present([true, false]));

        // A lane that is still there stays open.
        lanes.open(TimelineLane::Sound);
        assert!(!lanes.retain_present([true, true]));
        assert!(lanes.is_open(TimelineLane::Sound));
    }

    #[test]
    fn the_controls_share_one_lane_stack() {
        let lanes = Rc::new(RefCell::new(TimelineLanes::default()));
        let controls = LaneControls::new(lanes.clone());
        assert!(controls.is_open(TimelineLane::Zoom));
        controls.open(TimelineLane::Sound);
        assert!(controls.is_open(TimelineLane::Sound));
        assert!(lanes.borrow().is_open(TimelineLane::Sound));
    }
}
