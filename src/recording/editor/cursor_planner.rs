//! The studied cursor path planner.
//!
//! The reference does not run the cursor spring straight against the recorded
//! path. It groups the moments that must be visually exact (clicks, drags,
//! hover changes, cursor-image changes), searches a lead and a spring-speed
//! multiplier per group so the cursor meets those moments, merges nearby
//! groups when one plan cannot serve both, and then simulates the spring once
//! over the whole recording on a one-millisecond grid. This module is that
//! planner.

use super::cursor_path::{SpringConfig, REST_EPSILON, SPRING_BUCKET_MS};

const GROUP_GAP_MS: f64 = 100.0;
const MERGE_GAP_MS: f64 = 400.0;
const LEAD_DECAY_PER_MS: f64 = 0.35;
const MAX_COMPRESSION_RATE: f64 = 1.4;
const MAX_LEAD_MS: f64 = 600.0;
const APPROACH_WINDOW_MS: f64 = 1500.0;
const COMPRESSION_WINDOW_MS: f64 = 400.0;
const SPEED_LEVELS: [f64; 16] = [
    1.0, 1.25, 1.6, 2.0, 2.5, 3.2, 4.0, 5.0, 6.4, 8.0, 10.0, 13.0, 16.0, 20.0, 25.0, 32.0,
];
const SPEED_BLEND_IN_MS: f64 = 250.0;
const SPEED_BLEND_OUT_MS: f64 = 300.0;
const ACCEPTED_ERROR_RATIO: f64 = 0.75;
const KNOT_EPSILON_MS: f64 = 0.001;
const PATH_INTERPOLATION_GAP_MS: f64 = 40.0;
const MIN_LEAD_STEP_MS: f64 = 0.25;
const REFINE_LIMIT: usize = 6;
const MINIMIZE_LIMIT: usize = 12;

/// A recorded pointer position in milliseconds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlannerPoint {
    pub time_ms: f64,
    pub x: f64,
    pub y: f64,
}

/// A moment the plan must land accurately, with the reference's tolerance in
/// points: clicks at 2, drags at 2, hover changes at 4, cursor-image changes
/// at 6.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SyncPoint {
    pub time_ms: f64,
    pub tolerance: f64,
}

/// The planned cursor positions, sampled on a one-millisecond grid.
#[derive(Debug, Default)]
pub struct PlannedPath {
    runs: Vec<Run>,
    initial: (f64, f64),
}

#[derive(Debug)]
struct Run {
    start_ms: f64,
    xs: Vec<f32>,
    ys: Vec<f32>,
}

impl PlannedPath {
    pub fn position_at(&self, time_ms: f64) -> (f64, f64) {
        let Some(index) = self
            .runs
            .partition_point(|run| run.start_ms <= time_ms)
            .checked_sub(1)
        else {
            return self.initial;
        };
        let run = &self.runs[index];
        let offset = time_ms - run.start_ms;
        let last = run.xs.len().saturating_sub(1);
        if last == 0 || offset >= last as f64 {
            return (
                f64::from(*run.xs.last().unwrap_or(&0.0)),
                f64::from(*run.ys.last().unwrap_or(&0.0)),
            );
        }
        let base = offset.floor() as usize;
        let amount = offset - base as f64;
        let x = f64::from(run.xs[base])
            + (f64::from(run.xs[base + 1]) - f64::from(run.xs[base])) * amount;
        let y = f64::from(run.ys[base])
            + (f64::from(run.ys[base + 1]) - f64::from(run.ys[base])) * amount;
        (x, y)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct State {
    x: f64,
    y: f64,
    velocity_x: f64,
    velocity_y: f64,
}

impl State {
    fn at(x: f64, y: f64) -> Self {
        Self {
            x,
            y,
            velocity_x: 0.0,
            velocity_y: 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct MovingRange {
    start_ms: f64,
    end_ms: f64,
}

struct Path<'a> {
    points: &'a [PlannerPoint],
    moving: Vec<MovingRange>,
}

impl<'a> Path<'a> {
    fn new(points: &'a [PlannerPoint]) -> Self {
        let mut moving = Vec::new();
        for pair in points.windows(2) {
            let (from, to) = (pair[0], pair[1]);
            if to.x == from.x && to.y == from.y {
                continue;
            }
            let gap = to.time_ms - from.time_ms;
            let start = if gap <= PATH_INTERPOLATION_GAP_MS {
                from.time_ms
            } else {
                to.time_ms
            };
            if let Some(last) = moving.last_mut() {
                let last: &mut MovingRange = last;
                if last.end_ms >= start {
                    last.end_ms = last.end_ms.max(to.time_ms);
                    continue;
                }
            }
            moving.push(MovingRange {
                start_ms: start,
                end_ms: to.time_ms.max(start + 0.000001),
            });
        }
        Self { points, moving }
    }

    fn start_ms(&self) -> f64 {
        self.points
            .first()
            .map(|point| point.time_ms)
            .unwrap_or(0.0)
    }

    fn end_ms(&self) -> f64 {
        self.points.last().map(|point| point.time_ms).unwrap_or(0.0)
    }

    fn position_at(&self, time_ms: f64) -> (f64, f64) {
        let Some(first) = self.points.first() else {
            return (0.0, 0.0);
        };
        if time_ms <= first.time_ms {
            return (first.x, first.y);
        }
        let Some(last_point) = self.points.last() else {
            return (first.x, first.y);
        };
        if time_ms >= last_point.time_ms {
            return (last_point.x, last_point.y);
        }
        let index = self
            .points
            .partition_point(|point| point.time_ms <= time_ms)
            - 1;
        let from = self.points[index];
        let Some(to) = self.points.get(index + 1) else {
            return (from.x, from.y);
        };
        let span = to.time_ms - from.time_ms;
        if span <= 0.0 || span > PATH_INTERPOLATION_GAP_MS {
            return (from.x, from.y);
        }
        let amount = ((time_ms - from.time_ms) / span).clamp(0.0, 1.0);
        (
            from.x + (to.x - from.x) * amount,
            from.y + (to.y - from.y) * amount,
        )
    }

    fn is_still_between(&self, from_ms: f64, to_ms: f64) -> bool {
        let (from_ms, to_ms) = if to_ms < from_ms {
            (to_ms, from_ms)
        } else {
            (from_ms, to_ms)
        };
        let Some(index) = self
            .moving
            .partition_point(|range| range.start_ms <= to_ms)
            .checked_sub(1)
        else {
            return true;
        };
        self.moving[index].end_ms <= from_ms
    }

    fn last_rest_ending_by(&self, time_ms: f64) -> Option<MovingRange> {
        let index = self
            .moving
            .partition_point(|range| range.start_ms <= time_ms)
            .checked_sub(1)?;
        let range = self.moving[index];
        Some(MovingRange {
            start_ms: index
                .checked_sub(1)
                .map(|previous| self.moving[previous].end_ms)
                .unwrap_or(f64::NEG_INFINITY),
            end_ms: range.start_ms,
        })
    }
}

#[derive(Debug, Clone, Default)]
struct LeadKnots {
    knots: Vec<(f64, f64)>,
}

impl LeadKnots {
    fn add(&mut self, time_ms: f64, lead_ms: f64) {
        if let Some(last) = self.knots.last() {
            if time_ms < last.0 {
                return;
            }
        }
        self.knots.push((time_ms, lead_ms));
    }

    fn planned_at(&self, time_ms: f64) -> f64 {
        let Some(index) = self
            .knots
            .partition_point(|knot| knot.0 <= time_ms)
            .checked_sub(1)
        else {
            return 0.0;
        };
        let (from_time, from_lead) = self.knots[index];
        let Some(&(to_time, to_lead)) = self.knots.get(index + 1) else {
            return from_lead;
        };
        if to_time == from_time {
            return from_lead;
        }
        from_lead + (to_lead - from_lead) * (time_ms - from_time) / (to_time - from_time)
    }

    fn end_lead(&self) -> f64 {
        self.knots.last().map(|knot| knot.1).unwrap_or(0.0)
    }
}

#[derive(Debug, Clone, Copy)]
struct SpeedBump {
    ramp_in_start_ms: f64,
    full_start_ms: f64,
    full_end_ms: f64,
    ramp_out_end_ms: f64,
    speed: f64,
}

#[derive(Debug, Clone, Default)]
struct SpeedSchedule {
    bumps: Vec<SpeedBump>,
    longest_ms: f64,
}

impl SpeedSchedule {
    fn add(&mut self, bump: SpeedBump) {
        if bump.speed <= 1.0 {
            return;
        }
        self.bumps.push(bump);
        self.bumps
            .sort_by(|left, right| left.ramp_in_start_ms.total_cmp(&right.ramp_in_start_ms));
        self.longest_ms = self
            .longest_ms
            .max(bump.ramp_out_end_ms - bump.ramp_in_start_ms);
    }

    fn at(&self, time_ms: f64) -> f64 {
        if self.bumps.is_empty() {
            return 1.0;
        }
        let mut log_speed: f64 = 0.0;
        let mut index = self
            .bumps
            .partition_point(|bump| bump.ramp_in_start_ms <= time_ms);
        while index > 0 {
            index -= 1;
            let bump = self.bumps[index];
            if bump.ramp_in_start_ms < time_ms - self.longest_ms {
                break;
            }
            if bump.ramp_out_end_ms > time_ms {
                log_speed = log_speed.max(Self::log_at(bump, time_ms));
            }
        }
        log_speed.exp()
    }

    fn log_at(bump: SpeedBump, time_ms: f64) -> f64 {
        let log_speed = bump.speed.ln();
        if time_ms < bump.full_start_ms {
            let ramp = bump.full_start_ms - bump.ramp_in_start_ms;
            if ramp <= 0.0 {
                return log_speed;
            }
            return log_speed * (time_ms - bump.ramp_in_start_ms) / ramp;
        }
        if time_ms <= bump.full_end_ms {
            return log_speed;
        }
        let ramp = bump.ramp_out_end_ms - bump.full_end_ms;
        if ramp <= 0.0 {
            return 0.0;
        }
        log_speed * (bump.ramp_out_end_ms - time_ms) / ramp
    }
}

struct Simulation<'a> {
    path: &'a Path<'a>,
    spring: SpringConfig,
}

impl Simulation<'_> {
    fn lag_ms(&self) -> f64 {
        if self.spring.stiffness <= 0.0 {
            return 0.0;
        }
        self.spring.damping / self.spring.stiffness * 1000.0
    }

    fn target_at(&self, lead: &LeadKnots, time_ms: f64) -> (f64, f64) {
        self.path.position_at(time_ms + lead.planned_at(time_ms))
    }

    fn resting_at(&self, (x, y): (f64, f64)) -> State {
        State::at(x, y)
    }

    #[allow(clippy::too_many_arguments)]
    fn run(
        &self,
        lead: &LeadKnots,
        speed: &SpeedSchedule,
        from_ms: f64,
        to_ms: f64,
        initial: State,
        speed_scale: f64,
        mut sample: impl FnMut(f64, f64, f64),
    ) -> State {
        let mut state = initial;
        let mut clock = from_ms;
        let mut bucket = f64::NAN;
        let mut stiffness = 0.0;
        let mut damping = 0.0;
        let mut omega = 0.0;
        let mut critical = 0.0;
        let mass = self.spring.mass.max(0.0);
        while clock < to_ms {
            let next = to_ms.min(clock.floor() + 1.0);
            let step_ms = next - clock;
            let (target_x, target_y) = self.target_at(lead, next);
            if mass == 0.0 {
                state = State::at(target_x, target_y);
            } else if state.x != target_x
                || state.y != target_y
                || state.velocity_x != 0.0
                || state.velocity_y != 0.0
            {
                let current = (next / SPRING_BUCKET_MS).floor() * SPRING_BUCKET_MS;
                if current != bucket {
                    bucket = current;
                    let rate = speed.at(current) * speed_scale;
                    stiffness = self.spring.stiffness * rate * rate;
                    damping = self.spring.damping * rate;
                    omega = (stiffness / mass).max(0.0).sqrt();
                    critical = 2.0 * (mass * stiffness).max(0.0).sqrt();
                }
                let seconds = step_ms / 1000.0;
                let dx = target_x - state.x;
                let dy = target_y - state.y;
                let damping_x =
                    if state.velocity_x * dx > 0.0 && state.velocity_x.abs() > dx.abs() * omega {
                        critical
                    } else {
                        damping
                    };
                let damping_y =
                    if state.velocity_y * dy > 0.0 && state.velocity_y.abs() > dy.abs() * omega {
                        critical
                    } else {
                        damping
                    };
                state.velocity_x +=
                    (dx * stiffness - state.velocity_x * damping_x) / mass * seconds;
                state.velocity_y +=
                    (dy * stiffness - state.velocity_y * damping_y) / mass * seconds;
                state.x += state.velocity_x * seconds;
                state.y += state.velocity_y * seconds;
                if state.velocity_x.abs() < REST_EPSILON
                    && (state.x - target_x).abs() < REST_EPSILON
                {
                    state.x = target_x;
                    state.velocity_x = 0.0;
                }
                if state.velocity_y.abs() < REST_EPSILON
                    && (state.y - target_y).abs() < REST_EPSILON
                {
                    state.y = target_y;
                    state.velocity_y = 0.0;
                }
            }
            clock = next;
            if clock == clock.floor() {
                sample(clock, state.x, state.y);
            }
        }
        state
    }
}

#[derive(Clone, Copy)]
struct Group {
    start_ms: f64,
    end_ms: f64,
    first: usize,
    last: usize,
}

struct Context {
    anchor_ms: f64,
    anchor_lead_ms: f64,
    anchor_state: State,
    lead: LeadKnots,
    speed: SpeedSchedule,
    window_start_ms: f64,
}

struct Choice {
    speed: f64,
    error: f64,
    end_state: State,
    knots: Vec<(f64, f64)>,
    bump: Option<SpeedBump>,
}

struct LeadPlan {
    knots: Vec<(f64, f64)>,
    changes_from_ms: f64,
}

struct Record {
    group: Group,
    choice: Choice,
    lead_before: LeadKnots,
    speed_before: SpeedSchedule,
}

/// Plan the cursor path over the recording.
pub fn plan(
    points: &[PlannerPoint],
    sync: &[SyncPoint],
    spring: SpringConfig,
    speed_scale: f64,
    duration_ms: f64,
) -> PlannedPath {
    if points.len() < 2 {
        return PlannedPath {
            runs: Vec::new(),
            initial: points
                .first()
                .map(|point| (point.x, point.y))
                .unwrap_or((0.0, 0.0)),
        };
    }
    let path = Path::new(points);
    let simulation = Simulation {
        path: &path,
        spring,
    };
    let mut sorted = sync.to_vec();
    sorted.sort_by(|left, right| left.time_ms.total_cmp(&right.time_ms));
    let mut groups = group_sync_points(&sorted);
    let start = path.start_ms().min(0.0);
    let mut lead = LeadKnots::default();
    let mut speed = SpeedSchedule::default();
    let mut records: Vec<Record> = Vec::new();
    let mut index = 0;
    while index < groups.len() {
        let group = groups[index];
        let previous = records.last();
        let context = Context {
            anchor_ms: previous.map(|record| record.group.end_ms).unwrap_or(start),
            anchor_lead_ms: previous
                .map(|record| record.choice.knots.last().map(|knot| knot.1).unwrap_or(0.0))
                .unwrap_or(0.0),
            anchor_state: previous
                .map(|record| record.choice.end_state)
                .unwrap_or(simulation.resting_at(simulation.target_at(&lead, start))),
            lead: lead.clone(),
            speed: speed.clone(),
            window_start_ms: previous
                .map(|record| record.group.end_ms)
                .unwrap_or(start)
                .max(group.start_ms - APPROACH_WINDOW_MS - MAX_LEAD_MS)
                .floor(),
        };
        let choice = choose(&simulation, &path, &sorted, group, &context, speed_scale);
        if choice.error > 1.0 {
            if let Some(previous) = previous {
                if group.start_ms - previous.group.end_ms <= MERGE_GAP_MS {
                    let merged = Group {
                        start_ms: previous.group.start_ms,
                        end_ms: group.end_ms,
                        first: previous.group.first,
                        last: group.last,
                    };
                    let lead_before = previous.lead_before.clone();
                    let speed_before = previous.speed_before.clone();
                    records.pop();
                    lead = lead_before;
                    speed = speed_before;
                    groups[index] = merged;
                    continue;
                }
            }
        }
        let lead_before = lead.clone();
        let speed_before = speed.clone();
        for (time, value) in &choice.knots {
            lead.add(*time, *value);
        }
        if let Some(bump) = choice.bump {
            speed.add(bump);
        }
        records.push(Record {
            group,
            choice,
            lead_before,
            speed_before,
        });
        index += 1;
    }
    let last = records.last();
    add_default_knots(
        &path,
        &mut lead,
        last.map(|record| record.group.end_ms).unwrap_or(start),
        last.map(|record| record.choice.knots.last().map(|knot| knot.1).unwrap_or(0.0))
            .unwrap_or(0.0),
    );
    simulate(&simulation, &lead, &speed, duration_ms, speed_scale)
}

fn group_sync_points(sync: &[SyncPoint]) -> Vec<Group> {
    let mut groups: Vec<Group> = Vec::new();
    for (index, point) in sync.iter().enumerate() {
        if let Some(last) = groups.last_mut() {
            if point.time_ms - last.end_ms <= GROUP_GAP_MS {
                last.end_ms = point.time_ms;
                last.last = index;
                continue;
            }
        }
        groups.push(Group {
            start_ms: point.time_ms,
            end_ms: point.time_ms,
            first: index,
            last: index,
        });
    }
    groups
}

fn add_default_knots(path: &Path<'_>, lead: &mut LeadKnots, anchor_ms: f64, anchor_lead_ms: f64) {
    for (time, value) in default_knots(path, anchor_ms, anchor_lead_ms) {
        lead.add(time, value);
    }
}

fn default_knots(path: &Path<'_>, anchor_ms: f64, anchor_lead_ms: f64) -> Vec<(f64, f64)> {
    if anchor_lead_ms <= 0.0 {
        return vec![(anchor_ms, 0.0)];
    }
    let decay = anchor_lead_ms / LEAD_DECAY_PER_MS;
    let end = if path.is_still_between(anchor_ms, anchor_ms + anchor_lead_ms) {
        anchor_ms + KNOT_EPSILON_MS
    } else {
        anchor_ms + decay
    };
    vec![(anchor_ms, anchor_lead_ms), (end, 0.0)]
}

fn choose(
    simulation: &Simulation<'_>,
    path: &Path<'_>,
    sync: &[SyncPoint],
    group: Group,
    context: &Context,
    speed_scale: f64,
) -> Choice {
    let mut cache: Vec<Option<Choice>> = (0..SPEED_LEVELS.len()).map(|_| None).collect();
    let evaluate = |level: usize, cache: &mut Vec<Option<Choice>>| -> Option<Choice> {
        if cache[level].is_none() {
            cache[level] = choose_lead(
                simulation,
                path,
                sync,
                group,
                context,
                SPEED_LEVELS[level],
                speed_scale,
            );
        }
        cache[level].as_ref().map(|choice| Choice {
            speed: choice.speed,
            error: choice.error,
            end_state: choice.end_state,
            knots: choice.knots.clone(),
            bump: choice.bump,
        })
    };
    let Some(first) = evaluate(0, &mut cache) else {
        return fallback_choice(simulation, path, group, context);
    };
    if first.error <= ACCEPTED_ERROR_RATIO {
        return first;
    }
    let last_level = SPEED_LEVELS.len() - 1;
    let Some(last) = evaluate(last_level, &mut cache) else {
        return first;
    };
    if last.error > ACCEPTED_ERROR_RATIO {
        let mut best = first;
        for level in 0..=last_level {
            if let Some(choice) = evaluate(level, &mut cache) {
                if choice.error < best.error {
                    best = choice;
                }
            }
        }
        return best;
    }
    let (mut low, mut high) = (0usize, last_level);
    while high - low > 1 {
        let middle = (low + high) / 2;
        match evaluate(middle, &mut cache) {
            Some(choice) if choice.error <= ACCEPTED_ERROR_RATIO => high = middle,
            _ => low = middle,
        }
    }
    evaluate(high, &mut cache).unwrap_or(first)
}

fn fallback_choice(
    simulation: &Simulation<'_>,
    path: &Path<'_>,
    group: Group,
    context: &Context,
) -> Choice {
    let knots = default_knots(path, context.anchor_ms, context.anchor_lead_ms);
    Choice {
        speed: 1.0,
        error: f64::INFINITY,
        end_state: simulation.resting_at(simulation.target_at(&context.lead, group.end_ms)),
        knots,
        bump: None,
    }
}

fn choose_lead(
    simulation: &Simulation<'_>,
    path: &Path<'_>,
    sync: &[SyncPoint],
    group: Group,
    context: &Context,
    speed: f64,
    speed_scale: f64,
) -> Option<Choice> {
    let lag = simulation.lag_ms() / speed.max(1.0);
    let mut candidates = vec![0.0, MAX_LEAD_MS];
    let mut candidate = lag / 8.0;
    while candidate < MAX_LEAD_MS {
        if candidate > 0.0 {
            candidates.push(candidate);
        }
        candidate *= 2.0;
    }
    candidates.sort_by(|left, right| left.total_cmp(right));
    candidates.dedup();
    let mut tried: Vec<Option<Choice>> = Vec::new();
    let mut best_index: Option<usize> = None;
    for (index, lead) in candidates.iter().enumerate() {
        let limit = best_index
            .and_then(|best| {
                tried
                    .get(best)
                    .and_then(|choice| choice.as_ref())
                    .map(|c| c.error)
            })
            .unwrap_or(f64::INFINITY);
        let choice = try_knobs(
            simulation,
            path,
            sync,
            group,
            context,
            *lead,
            speed,
            speed_scale,
            limit.max(ACCEPTED_ERROR_RATIO),
        );
        tried.push(choice);
        let Some(current) = tried[index].as_ref() else {
            continue;
        };
        if current.error <= ACCEPTED_ERROR_RATIO {
            if index == 0 {
                return tried[index].take();
            }
            let previous = candidates[index - 1];
            return refine_lead(
                simulation,
                path,
                sync,
                group,
                context,
                previous,
                *lead,
                speed,
                speed_scale,
            );
        }
        if best_index
            .and_then(|best| tried[best].as_ref().map(|choice| choice.error))
            .map(|best| current.error < best)
            .unwrap_or(true)
        {
            best_index = Some(index);
        }
    }
    match best_index {
        None => try_knobs(
            simulation,
            path,
            sync,
            group,
            context,
            0.0,
            speed,
            speed_scale,
            f64::INFINITY,
        )
        .or_else(|| tried.into_iter().next().flatten()),
        Some(index) => {
            let low = candidates[index.saturating_sub(1)];
            let high = candidates[(index + 1).min(candidates.len() - 1)];
            let best = tried[index].take()?;
            minimize_lead(
                simulation,
                path,
                sync,
                group,
                context,
                low,
                high,
                best,
                speed,
                speed_scale,
            )
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn refine_lead(
    simulation: &Simulation<'_>,
    path: &Path<'_>,
    sync: &[SyncPoint],
    group: Group,
    context: &Context,
    mut low: f64,
    mut high: f64,
    speed: f64,
    speed_scale: f64,
) -> Option<Choice> {
    let mut best = try_knobs(
        simulation,
        path,
        sync,
        group,
        context,
        high,
        speed,
        speed_scale,
        f64::INFINITY,
    )?;
    for _ in 0..REFINE_LIMIT {
        if high - low <= 0.5 {
            break;
        }
        let middle = (low + high) / 2.0;
        match try_knobs(
            simulation,
            path,
            sync,
            group,
            context,
            middle,
            speed,
            speed_scale,
            f64::INFINITY,
        ) {
            Some(choice) if choice.error <= ACCEPTED_ERROR_RATIO => {
                best = choice;
                high = middle;
            }
            _ => low = middle,
        }
    }
    Some(best)
}

#[allow(clippy::too_many_arguments)]
fn minimize_lead(
    simulation: &Simulation<'_>,
    path: &Path<'_>,
    sync: &[SyncPoint],
    group: Group,
    context: &Context,
    mut low: f64,
    mut high: f64,
    mut best: Choice,
    speed: f64,
    speed_scale: f64,
) -> Option<Choice> {
    let ratio = (5.0_f64.sqrt() - 1.0) / 2.0;
    for _ in 0..MINIMIZE_LIMIT {
        if high - low <= MIN_LEAD_STEP_MS {
            break;
        }
        let left = high - (high - low) * ratio;
        let right = low + (high - low) * ratio;
        let left_choice = try_knobs(
            simulation,
            path,
            sync,
            group,
            context,
            left,
            speed,
            speed_scale,
            f64::INFINITY,
        );
        let right_choice = try_knobs(
            simulation,
            path,
            sync,
            group,
            context,
            right,
            speed,
            speed_scale,
            f64::INFINITY,
        );
        let left_error = left_choice
            .as_ref()
            .map(|choice| choice.error)
            .unwrap_or(f64::INFINITY);
        let right_error = right_choice
            .as_ref()
            .map(|choice| choice.error)
            .unwrap_or(f64::INFINITY);
        if let Some(choice) = left_choice {
            if choice.error < best.error {
                best = choice;
            }
        }
        if let Some(choice) = right_choice {
            if choice.error < best.error {
                best = choice;
            }
        }
        if best.error <= ACCEPTED_ERROR_RATIO {
            return Some(best);
        }
        if left_error < right_error {
            high = right;
        } else {
            low = left;
        }
    }
    Some(best)
}

#[allow(clippy::too_many_arguments)]
fn try_knobs(
    simulation: &Simulation<'_>,
    path: &Path<'_>,
    sync: &[SyncPoint],
    group: Group,
    context: &Context,
    lead_ms: f64,
    speed: f64,
    speed_scale: f64,
    limit: f64,
) -> Option<Choice> {
    let lead_plan = lead_plan(path, group, context, lead_ms)?;
    let mut lead = context.lead.clone();
    for (time, value) in &lead_plan.knots {
        lead.add(*time, *value);
    }
    let mut schedule = context.speed.clone();
    let bump = if speed > 1.0 {
        Some(SpeedBump {
            ramp_in_start_ms: context.anchor_ms.max(group.start_ms - SPEED_BLEND_IN_MS),
            full_start_ms: group.start_ms,
            full_end_ms: group.end_ms,
            ramp_out_end_ms: group.end_ms + SPEED_BLEND_OUT_MS,
            speed,
        })
    } else {
        None
    };
    if let Some(bump) = bump {
        schedule.add(bump);
    }
    let changes_from = bump
        .map(|bump| bump.ramp_in_start_ms)
        .unwrap_or(f64::INFINITY)
        .min(lead_plan.changes_from_ms)
        .min(group.start_ms);
    let window_start = context.window_start_ms.max(changes_from.floor());
    let mut state = committed_state_at(simulation, path, context, window_start, speed_scale);
    let mut clock = window_start;
    let mut error: f64 = 0.0;
    for point in &sync[group.first..=group.last] {
        state = simulation.run(
            &lead,
            &schedule,
            clock,
            point.time_ms,
            state,
            speed_scale,
            |_, _, _| {},
        );
        clock = point.time_ms;
        let (target_x, target_y) = path.position_at(point.time_ms);
        let distance = (state.x - target_x).hypot(state.y - target_y);
        let tolerance = point.tolerance.max(f64::EPSILON);
        error = error.max(distance / tolerance);
        if error > limit {
            break;
        }
    }
    Some(Choice {
        speed,
        error,
        end_state: state,
        knots: lead_plan.knots,
        bump,
    })
}

fn committed_state_at(
    simulation: &Simulation<'_>,
    path: &Path<'_>,
    context: &Context,
    window_start_ms: f64,
    speed_scale: f64,
) -> State {
    let mut lead = context.lead.clone();
    add_default_knots(path, &mut lead, context.anchor_ms, context.anchor_lead_ms);
    simulation.run(
        &lead,
        &context.speed,
        context.anchor_ms,
        window_start_ms,
        context.anchor_state,
        speed_scale,
        |_, _, _| {},
    )
}

fn lead_plan(path: &Path<'_>, group: Group, context: &Context, lead_ms: f64) -> Option<LeadPlan> {
    let mut planned = LeadKnots::default();
    for (time, value) in default_knots(path, context.anchor_ms, context.anchor_lead_ms) {
        planned.add(time, value);
    }
    if lead_ms <= 0.0 {
        let mut knots: Vec<(f64, f64)> = planned
            .knots
            .iter()
            .copied()
            .filter(|(time, _)| *time < group.end_ms)
            .collect();
        knots.push((group.end_ms, planned.planned_at(group.end_ms)));
        return Some(LeadPlan {
            knots,
            changes_from_ms: group.start_ms,
        });
    }
    let raise = find_lead_raise(path, group, context, lead_ms, &planned)?;
    let mut knots: Vec<(f64, f64)> = planned
        .knots
        .iter()
        .copied()
        .filter(|(time, _)| *time < raise.at_ms)
        .collect();
    knots.push((raise.at_ms, planned.planned_at(raise.at_ms)));
    knots.push((raise.at_ms + KNOT_EPSILON_MS, raise.jump_to_ms));
    knots.push((group.start_ms, lead_ms));
    knots.push((group.end_ms, lead_ms));
    Some(LeadPlan {
        knots,
        changes_from_ms: raise.at_ms,
    })
}

struct LeadRaise {
    at_ms: f64,
    jump_to_ms: f64,
}

fn find_lead_raise(
    path: &Path<'_>,
    group: Group,
    context: &Context,
    lead_ms: f64,
    planned: &LeadKnots,
) -> Option<LeadRaise> {
    let acceptable = |offset_ms: f64, duration_ms: f64| {
        let denominator = group.start_ms - offset_ms;
        if denominator <= 0.0 {
            return false;
        }
        let rate = (group.start_ms + lead_ms - (offset_ms + duration_ms)) / denominator;
        (1.0 - LEAD_DECAY_PER_MS..=MAX_COMPRESSION_RATE).contains(&rate)
    };
    let approach_start = context.anchor_ms.max(group.start_ms - APPROACH_WINDOW_MS);
    let mut rest = path.last_rest_ending_by(group.start_ms + lead_ms);
    while let Some(range) = rest {
        if range.end_ms <= approach_start {
            break;
        }
        let at_ms = (group.start_ms - 1.0).min(approach_start.max(range.end_ms - lead_ms));
        let jump = planned.planned_at(at_ms);
        let jump_clamped = lead_ms.min(range.end_ms - at_ms);
        if at_ms + jump_clamped.min(jump) >= range.start_ms
            && at_ms + jump_clamped.max(jump) <= range.end_ms
            && acceptable(at_ms, jump_clamped)
        {
            return Some(LeadRaise {
                at_ms,
                jump_to_ms: jump_clamped,
            });
        }
        rest = path.last_rest_ending_by(range.start_ms);
    }
    let compression_start = context
        .anchor_ms
        .max(group.start_ms - COMPRESSION_WINDOW_MS);
    if compression_start >= group.start_ms {
        return None;
    }
    let jump = planned.planned_at(compression_start);
    if acceptable(compression_start, jump) {
        return Some(LeadRaise {
            at_ms: compression_start,
            jump_to_ms: jump,
        });
    }
    None
}

fn simulate(
    simulation: &Simulation<'_>,
    lead: &LeadKnots,
    speed: &SpeedSchedule,
    duration_ms: f64,
    speed_scale: f64,
) -> PlannedPath {
    let start = simulation.path.start_ms().min(0.0).floor();
    let initial_target = simulation.target_at(lead, start);
    let end = start.max(duration_ms.ceil());
    let mut runs: Vec<Run> = Vec::new();
    let mut current: Option<Run> = None;
    let mut last = initial_target;
    simulation.run(
        lead,
        speed,
        start,
        end,
        State::at(initial_target.0, initial_target.1),
        speed_scale,
        |time, x, y| {
            if x == last.0 && y == last.1 {
                if let Some(run) = current.take() {
                    runs.push(run);
                }
                return;
            }
            let run = current.get_or_insert_with(|| Run {
                start_ms: time,
                xs: Vec::new(),
                ys: Vec::new(),
            });
            run.xs.push(x as f32);
            run.ys.push(y as f32);
            last = (x, y);
        },
    );
    if let Some(run) = current {
        runs.push(run);
    }
    PlannedPath {
        runs,
        initial: initial_target,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sweep(steps: usize, step_x: f64) -> Vec<PlannerPoint> {
        (0..=steps)
            .map(|index| PlannerPoint {
                time_ms: index as f64 * 8.0,
                x: index as f64 * step_x,
                y: 0.0,
            })
            .collect()
    }

    #[test]
    fn plain_movement_trails_and_lands() {
        let points = sweep(50, 4.0);
        let path = plan(&points, &[], SpringConfig::NONE, 1.0, 1400.0);
        let (moving, _) = path.position_at(200.0);
        let (raw, _) = (200.0 / 400.0 * 200.0, 0.0);
        assert!(moving > 0.0 && moving < raw, "plain movement trails");
        let (end, _) = path.position_at(1400.0);
        assert!((end - 200.0).abs() < 1.0, "the cursor should land");
    }

    #[test]
    fn a_click_pulls_the_cursor_onto_its_point() {
        let points = sweep(50, 6.0);
        let click = SyncPoint {
            time_ms: 400.0,
            tolerance: 2.0,
        };
        let planned = plan(&points, &[click], SpringConfig::NONE, 1.0, 1400.0);
        let plain = plan(&points, &[], SpringConfig::NONE, 1.0, 1400.0);
        let (met, _) = planned.position_at(400.0);
        let (trailed, _) = plain.position_at(400.0);
        assert!(
            (met - 300.0).abs() <= 2.0,
            "the click point should be met, got {met}"
        );
        assert!(
            (trailed - 300.0).abs() > 5.0,
            "without a sync the cursor trails, got {trailed}"
        );
    }

    #[test]
    fn nearby_clicks_merge_into_one_plan() {
        let points = sweep(60, 5.0);
        let sync = [
            SyncPoint {
                time_ms: 240.0,
                tolerance: 2.0,
            },
            SyncPoint {
                time_ms: 300.0,
                tolerance: 2.0,
            },
        ];
        let path = plan(&points, &sync, SpringConfig::RAPID, 1.0, 1400.0);
        let (first, _) = path.position_at(240.0);
        let (second, _) = path.position_at(300.0);
        assert!(first.is_finite() && second.is_finite());
        assert!(second > first, "the cursor should be moving between clicks");
    }

    #[test]
    fn a_long_rest_holds_the_final_point() {
        let mut points = sweep(20, 5.0);
        points.push(PlannerPoint {
            time_ms: 3000.0,
            x: 100.0,
            y: 0.0,
        });
        let path = plan(&points, &[], SpringConfig::NONE, 1.0, 4000.0);
        let (rest, _) = path.position_at(2000.0);
        assert!((rest - 100.0).abs() < 0.5, "the cursor should be at rest");
    }
}
