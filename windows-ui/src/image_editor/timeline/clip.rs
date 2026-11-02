//! Clip hit testing, context actions and edge trimming.

use super::*;
use crate::capture::editor::window::icon_names;
use crate::recording::editor::model::MIN_MOTION_SEGMENT_SECONDS;
use gtk4::{gdk, Popover, PositionType};

#[derive(Clone)]
pub(super) struct Hooks {
    pub shared: Rc<Shared>,
    pub redraw: Rc<dyn Fn()>,
    pub pause: Rc<dyn Fn()>,
    pub start_playback: Rc<dyn Fn(Option<PreviewRange>)>,
    pub dragged: Rc<Cell<bool>>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Edge {
    Start,
    End,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Trim {
    text: bool,
    index: usize,
    edge: Edge,
    origin: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Action {
    Duplicate,
    ToggleVisibility,
    Delete,
}

fn count(model: &Model, text: bool) -> usize {
    if text {
        model.text_segments.len()
    } else {
        model.segments.len()
    }
}

fn bounds(model: &Model, text: bool, index: usize) -> Option<(f64, f64)> {
    if text {
        model.text_segments.get(index).map(|s| (s.start, s.end))
    } else {
        model.segments.get(index).map(|s| (s.start, s.end))
    }
}

fn select(model: &mut Model, text: bool, index: usize) -> bool {
    if bounds(model, text, index).is_none() {
        return false;
    }
    model.selected = (!text).then_some(index);
    model.selected_text = text.then_some(index);
    true
}

fn inside_lane(text: bool, y: f64, height: f64) -> bool {
    let inset = if text { 6.0 } else { 7.0 };
    y >= inset && y <= height - inset
}

fn painted_bounds(model: &Model, text: bool, index: usize, width: f64) -> Option<(f64, f64)> {
    let (start, end) = bounds(model, text, index)?;
    let left = time_to_x(start, model.duration, width);
    let right = time_to_x(end, model.duration, width);
    Some((left, left + (right - left).max(22.0)))
}

fn clip_at(model: &Model, text: bool, x: f64, y: f64, width: f64, height: f64) -> Option<usize> {
    if !inside_lane(text, y, height) {
        return None;
    }
    let time = (x / width.max(1.0) * model.duration).clamp(0.0, model.duration);
    let actual = if text {
        model.text_index_at(time)
    } else {
        model.segment_index_at(time)
    };
    actual.or_else(|| {
        (0..count(model, text)).rev().find(|&index| {
            painted_bounds(model, text, index, width)
                .is_some_and(|(left, right)| x >= left && x <= right)
        })
    })
}

fn edge_at(
    model: &Model,
    text: bool,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    scale: f64,
) -> Option<Trim> {
    if !inside_lane(text, y, height) {
        return None;
    }
    let mut best: Option<(bool, f64, Trim)> = None;
    for index in 0..count(model, text) {
        let (start, end) = bounds(model, text, index)?;
        let (left, right) = painted_bounds(model, text, index, width)?;
        for (edge, position, origin) in [(Edge::Start, left, start), (Edge::End, right, end)] {
            let distance = (x - position).abs();
            if distance > 8.0 * scale {
                continue;
            }
            let inside = x >= left && x <= right;
            let replace = best.as_ref().is_none_or(|(old_inside, old_distance, _)| {
                (inside && !old_inside) || (inside == *old_inside && distance < *old_distance)
            });
            if replace {
                best = Some((
                    inside,
                    distance,
                    Trim {
                        text,
                        index,
                        edge,
                        origin,
                    },
                ));
            }
        }
    }
    best.map(|(_, _, trim)| trim)
}

fn resize(model: &mut Model, trim: Trim, offset: f64, width: f64, scale: f64) -> bool {
    let Some((start, end)) = bounds(model, trim.text, trim.index) else {
        return false;
    };
    let duration = model.duration;
    let time = trim.origin + offset / width.max(1.0) * duration;
    let tolerance = 10.0 * scale / width.max(1.0) * duration;
    let time = if trim.text {
        model.snap_text_time(time, tolerance, Some(trim.index))
    } else {
        model.snap_effect_time(time, tolerance, Some(trim.index))
    };
    let mut left: f64 = 0.0;
    let mut right = duration;
    for index in 0..count(model, trim.text) {
        if index == trim.index {
            continue;
        }
        if let Some((other_start, other_end)) = bounds(model, trim.text, index) {
            if other_end <= start + 1e-9 {
                left = left.max(other_end);
            }
            if other_start >= end - 1e-9 {
                right = right.min(other_start);
            }
        }
    }
    let (new_start, new_end) = match trim.edge {
        Edge::Start if end - MIN_MOTION_SEGMENT_SECONDS >= left => {
            (time.clamp(left, end - MIN_MOTION_SEGMENT_SECONDS), end)
        }
        Edge::End if start + MIN_MOTION_SEGMENT_SECONDS <= right => {
            (start, time.clamp(start + MIN_MOTION_SEGMENT_SECONDS, right))
        }
        _ => return false,
    };
    if (new_start - start).abs() < 1e-9 && (new_end - end).abs() < 1e-9 {
        return false;
    }
    if trim.text {
        model.resize_text_range(trim.index, new_start, new_end);
    } else {
        model.resize_segment_range(trim.index, new_start, new_end);
    }
    bounds(model, trim.text, trim.index).is_some_and(|(actual_start, actual_end)| {
        (actual_start - start).abs() > 1e-9 || (actual_end - end).abs() > 1e-9
    })
}

fn apply_action(model: &mut Model, text: bool, index: usize, action: Action) -> bool {
    if !select(model, text, index) {
        return false;
    }
    match action {
        Action::Duplicate => {
            if text {
                model.duplicate_text_segment(index).is_some()
            } else {
                model.duplicate_motion_segment(index).is_some()
            }
        }
        Action::ToggleVisibility => {
            if text {
                let disabled = model.text_segments[index].is_disabled;
                model.set_selected_text_disabled(!disabled);
            } else {
                let disabled = model.segments[index].is_disabled;
                model.set_selected_disabled(!disabled);
            }
            true
        }
        Action::Delete => model.remove_selected(),
    }
}

pub(super) fn install(row: &DrawingArea, text: bool, scale: f64, hooks: Hooks) {
    let right_click = gtk4::GestureClick::new();
    right_click.set_button(3);
    right_click.connect_pressed({
        let row = row.downgrade();
        let hooks = hooks.clone();
        move |_, _, x, y| {
            let Some(row) = row.upgrade() else { return };
            let index = {
                let mut model = hooks.shared.model.borrow_mut();
                let index = clip_at(
                    &model,
                    text,
                    x,
                    y,
                    f64::from(row.width().max(1)),
                    f64::from(row.height()),
                );
                if let Some(index) = index {
                    select(&mut model, text, index);
                }
                index
            };
            let Some(index) = index else { return };
            (hooks.redraw)();
            hooks.shared.notify_state();
            show_menu(&row, text, index, x, y, scale, &hooks);
        }
    });
    row.add_controller(right_click);

    let active = Rc::new(Cell::new(None::<Trim>));
    let checkpoint = Rc::new(RefCell::new(None::<Model>));
    let drag = gtk4::GestureDrag::new();
    drag.set_button(1);
    drag.connect_drag_begin({
        let row = row.downgrade();
        let active = active.clone();
        let hooks = hooks.clone();
        move |_, x, y| {
            let Some(row) = row.upgrade() else { return };
            let trim = edge_at(
                &hooks.shared.model.borrow(),
                text,
                x,
                y,
                f64::from(row.width().max(1)),
                f64::from(row.height()),
                scale,
            );
            active.set(trim);
            if let Some(trim) = trim {
                hooks.dragged.set(true);
                hooks.shared.dragging_clip.set(true);
                (hooks.pause)();
                select(&mut hooks.shared.model.borrow_mut(), text, trim.index);
                (hooks.redraw)();
                hooks.shared.notify_state();
            }
        }
    });
    drag.connect_drag_update({
        let row = row.downgrade();
        let active = active.clone();
        let checkpoint = checkpoint.clone();
        let shared = hooks.shared.clone();
        move |_, dx, _| {
            if !shared.dragging_clip.get() {
                return;
            }
            let (Some(row), Some(trim)) = (row.upgrade(), active.get()) else {
                return;
            };
            let mut model = shared.model.borrow_mut();
            let before = model.clone();
            if resize(&mut model, trim, dx, f64::from(row.width().max(1)), scale) {
                if checkpoint.borrow().is_none() {
                    *checkpoint.borrow_mut() = Some(before);
                }
                row.queue_draw();
            }
        }
    });
    let finish: Rc<dyn Fn()> = {
        let active = active.clone();
        let checkpoint = checkpoint.clone();
        let hooks = hooks.clone();
        Rc::new(move || {
            let Some(trim) = active.take() else { return };
            if !hooks.shared.dragging_clip.replace(false) {
                checkpoint.borrow_mut().take();
                hooks.dragged.set(false);
                return;
            }
            let before = checkpoint.borrow_mut().take();
            let changed = before.is_some();
            if let Some(before) = before {
                hooks.shared.record_edit(before);
            }
            (hooks.redraw)();
            hooks.shared.notify_preview();
            hooks.shared.notify_state();
            if changed && !trim.text {
                let range = hooks
                    .shared
                    .model
                    .borrow()
                    .selected_segment()
                    .map(motion_preview_range);
                if let Some(range) = range {
                    (hooks.start_playback)(Some(range));
                }
            }
            let dragged = hooks.dragged.clone();
            glib::idle_add_local_once(move || dragged.set(false));
        })
    };
    drag.connect_drag_end({
        let finish = finish.clone();
        move |_, _, _| finish()
    });
    drag.connect_cancel(move |_, _| finish());
    row.add_controller(drag);

    let pointer = gtk4::EventControllerMotion::new();
    pointer.connect_motion({
        let shared = hooks.shared.clone();
        move |controller, x, y| {
            let Some(widget) = controller.widget() else {
                return;
            };
            if shared.dragging_clip.get() {
                return;
            }
            let edge = edge_at(
                &shared.model.borrow(),
                text,
                x,
                y,
                f64::from(widget.width().max(1)),
                f64::from(widget.height()),
                scale,
            );
            widget.set_cursor_from_name(edge.map(|trim| match trim.edge {
                Edge::Start => "w-resize",
                Edge::End => "e-resize",
            }));
        }
    });
    pointer.connect_leave(|controller| {
        if let Some(widget) = controller.widget() {
            widget.set_cursor(None);
        }
    });
    row.add_controller(pointer);
}

fn show_menu(
    row: &DrawingArea,
    text: bool,
    index: usize,
    x: f64,
    y: f64,
    scale: f64,
    hooks: &Hooks,
) {
    let popover = build_menu(text, index, scale, hooks);
    popover.set_parent(row);
    popover.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
    popover.connect_closed(|popover| popover.unparent());
    popover.popup();
}

fn build_menu(text: bool, index: usize, scale: f64, hooks: &Hooks) -> Popover {
    let popover = Popover::new();
    popover.add_css_class("history-action-popover");
    popover.add_css_class("settings-select-popover");
    popover.add_css_class("image-editor-clip-menu");
    popover.set_has_arrow(false);
    popover.set_autohide(true);
    popover.set_position(PositionType::Top);
    let scale = scale.min(1.0);
    let menu = GtkBox::new(Orientation::Vertical, px(2.0, scale));
    menu.add_css_class("settings-select-menu");
    menu.set_size_request(px(160.0, scale), -1);
    let model = hooks.shared.model.borrow();
    let (disabled, can_duplicate) = if text {
        (
            model
                .text_segments
                .get(index)
                .is_some_and(|segment| segment.is_disabled),
            model.text_duplicate_span(index).is_some(),
        )
    } else {
        (
            model
                .segments
                .get(index)
                .is_some_and(|segment| segment.is_disabled),
            model.motion_duplicate_span(index).is_some(),
        )
    };
    drop(model);
    let source_revision = hooks.shared.source_revision.get();
    let items = [
        (
            Action::Duplicate,
            icon_names::shipped::COPY_ARROW_RIGHT_REGULAR,
            "Duplicate",
            can_duplicate,
        ),
        (
            Action::ToggleVisibility,
            if disabled {
                icon_names::shipped::EYE_REGULAR
            } else {
                icon_names::shipped::EYE_OFF_REGULAR
            },
            if disabled { "Show" } else { "Hide" },
            true,
        ),
        (Action::Delete, custom::USER_TRASH_SYMBOLIC, "Delete", true),
    ];
    for (action, icon, label, sensitive) in items {
        let button = Button::new();
        button.add_css_class("history-action-btn");
        button.set_has_frame(false);
        button.set_focus_on_click(false);
        button.set_sensitive(sensitive);
        button.set_size_request(-1, px(32.0, scale));
        let content = GtkBox::new(Orientation::Horizontal, px(10.0, scale));
        let image = Image::from_icon_name(icon);
        image.set_pixel_size(px(15.0, scale));
        let label = Label::new(Some(&t(label)));
        label.set_xalign(0.0);
        label.set_hexpand(true);
        content.append(&image);
        content.append(&label);
        button.set_child(Some(&content));
        button.connect_clicked({
            let popover = popover.downgrade();
            let hooks = hooks.clone();
            move |_| {
                if let Some(popover) = popover.upgrade() {
                    popover.popdown();
                }
                if hooks.shared.source_revision.get() != source_revision {
                    return;
                }
                (hooks.pause)();
                let mut model = hooks.shared.model.borrow_mut();
                let before = model.clone();
                let changed = apply_action(&mut model, text, index, action);
                drop(model);
                if changed {
                    hooks.shared.record_edit(before);
                    (hooks.redraw)();
                    hooks.shared.notify_preview();
                    hooks.shared.notify_state();
                }
            }
        });
        menu.append(&button);
    }
    popover.set_child(Some(&menu));
    popover
}

#[cfg(test)]
mod tests {
    use super::*;

    fn add(model: &mut Model, text: bool, at: f64) {
        if text {
            model.add_text_at(at).unwrap();
        } else {
            model.add_segment_at(at).unwrap();
        }
    }

    #[test]
    fn both_lanes_trim_with_snapping_neighbors_and_minimum_duration() {
        for text in [false, true] {
            let mut model = Model::default();
            add(&mut model, text, 1.0);
            add(&mut model, text, 4.0);
            let end = edge_at(&model, text, 200.0, 20.0, 600.0, 44.0, 1.0).unwrap();
            assert_eq!(end.edge, Edge::End);
            assert!(resize(&mut model, end, 350.0, 600.0, 1.0));
            assert_eq!(bounds(&model, text, 0), Some((1.0, 4.0)));
            assert_eq!(bounds(&model, text, 1), Some((4.0, 5.0)));
            let start = Trim {
                text,
                index: 0,
                edge: Edge::Start,
                origin: 1.0,
            };
            assert!(resize(&mut model, start, -200.0, 600.0, 1.0));
            assert_eq!(bounds(&model, text, 0), Some((0.0, 4.0)));
            let end = Trim { origin: 4.0, ..end };
            assert!(resize(&mut model, end, -900.0, 600.0, 1.0));
            assert_eq!(
                bounds(&model, text, 0),
                Some((0.0, MIN_MOTION_SEGMENT_SECONDS))
            );
            assert!(!resize(&mut model, end, -900.0, 600.0, 1.0));
            assert_eq!(
                if text {
                    model.selected_text
                } else {
                    model.selected
                },
                Some(0)
            );
        }

        let mut model = Model::default();
        add(&mut model, false, 1.0);
        add(&mut model, true, 3.0);
        let trim = Trim {
            text: false,
            index: 0,
            edge: Edge::End,
            origin: 2.0,
        };
        assert!(resize(&mut model, trim, 96.0, 600.0, 1.0));
        assert_eq!(model.segments[0].end, 3.0);
    }

    #[test]
    fn trimming_retimes_motion_and_text_animations() {
        let mut model = Model::default();
        add(&mut model, false, 1.0);
        add(&mut model, true, 1.0);
        model.segments[0].timing.transition_duration = 0.5;
        model.text_segments[0].transition_duration = 0.4;
        model.text_segments[0].typewriter_time = 0.6;
        for text in [false, true] {
            let trim = Trim {
                text,
                index: 0,
                edge: Edge::End,
                origin: 2.0,
            };
            assert!(resize(&mut model, trim, 100.0, 600.0, 1.0));
        }
        assert_eq!(model.segments[0].timing.transition_duration, 1.0);
        assert_eq!(model.text_segments[0].transition_duration, 0.8);
        assert_eq!(model.text_segments[0].typewriter_time, 1.2);
    }

    #[test]
    fn hit_tests_use_painted_edges_and_keep_adjacent_clips_distinct() {
        let mut model = Model::default();
        add(&mut model, false, 1.0);
        add(&mut model, false, 2.0);
        assert_eq!(
            edge_at(&model, false, 199.0, 20.0, 600.0, 56.0, 1.0)
                .unwrap()
                .index,
            0
        );
        let edge = edge_at(&model, false, 201.0, 20.0, 600.0, 56.0, 1.0).unwrap();
        assert_eq!((edge.index, edge.edge), (1, Edge::Start));
        assert!(edge_at(&model, false, 200.0, 2.0, 600.0, 56.0, 1.0).is_none());
        assert_eq!(clip_at(&model, false, 250.0, 20.0, 600.0, 56.0), Some(1));
        assert_eq!(clip_at(&model, false, 250.0, 2.0, 600.0, 56.0), None);
        model.resize_segment_range(0, 1.0, 1.25);
        let edge = edge_at(&model, false, 48.0, 20.0, 200.0, 56.0, 1.0).unwrap();
        assert_eq!(edge.edge, Edge::End);
        assert!(resize(&mut model, edge, 10.0, 200.0, 1.0));
        assert!((model.segments[0].end - 1.55).abs() < 1e-9);
    }

    #[test]
    fn menu_actions_use_shared_duplicate_visibility_and_delete_rules() {
        for text in [false, true] {
            let mut model = Model::default();
            add(&mut model, text, 1.0);
            if text {
                assert!(model.text_segments[0].sample(1.5).is_some());
            } else {
                assert!(model.sample(1.5).scale > 1.0);
            }
            assert!(apply_action(&mut model, text, 0, Action::ToggleVisibility));
            if text {
                assert!(model.text_segments[0].sample(1.5).is_none());
            } else {
                assert_eq!(model.sample(1.5).scale, 1.0);
            }
            let disabled = if text {
                model.text_segments[0].is_disabled
            } else {
                model.segments[0].is_disabled
            };
            assert!(disabled);
            assert!(apply_action(&mut model, text, 0, Action::Duplicate));
            assert_eq!(bounds(&model, text, 1), Some((2.0, 3.0)));
            assert!(apply_action(&mut model, text, 1, Action::ToggleVisibility));
            let disabled = if text {
                model.text_segments[1].is_disabled
            } else {
                model.segments[1].is_disabled
            };
            assert!(!disabled);
            assert!(apply_action(&mut model, text, 1, Action::Delete));
            assert_eq!(count(&model, text), 1);
            assert!(!apply_action(&mut model, text, 99, Action::Delete));
        }
    }

    #[test]
    fn drag_and_menu_widgets_update_history_for_both_lanes() {
        let Some(()) = crate::test_support::with_gtk(|| {
            for text in [false, true] {
                let timeline = super::super::build(1.0, true);
                {
                    let mut model = timeline.shared.model.borrow_mut();
                    add(&mut model, text, 1.0);
                }
                timeline.dock.measure(Orientation::Horizontal, -1);
                let height = timeline.dock.measure(Orientation::Vertical, 600).0.max(320);
                timeline.dock.allocate(600, height, -1, None);
                let card = timeline.dock.first_child().unwrap();
                let toolbar = card.first_child().unwrap();
                let board = card.last_child().unwrap().downcast::<Overlay>().unwrap();
                let tracks = board.child().unwrap();
                let ruler = tracks.first_child().unwrap();
                let source = ruler.next_sibling().unwrap();
                let motion = source.next_sibling().unwrap();
                let row = if text {
                    motion.next_sibling().unwrap()
                } else {
                    motion
                };
                let width = f64::from(row.width());
                let y = f64::from(row.height()) / 2.0;
                let drag = (0..row.observe_controllers().n_items())
                    .filter_map(|index| row.observe_controllers().item(index))
                    .find_map(|item| item.downcast::<gtk4::GestureDrag>().ok())
                    .unwrap();
                let x = width / 3.0;
                drag.emit_by_name::<()>("drag-begin", &[&x, &y]);
                assert!(timeline.shared.dragging_clip.get());
                drag.emit_by_name::<()>("drag-update", &[&(width / 12.0), &0.0_f64]);
                drag.emit_by_name::<()>("drag-update", &[&(width / 6.0), &0.0_f64]);
                drag.emit_by_name::<()>("drag-end", &[&(width / 6.0), &0.0_f64]);
                assert!(!timeline.shared.dragging_clip.get());
                assert!(
                    (bounds(&timeline.shared.model.borrow(), text, 0).unwrap().1 - 3.0).abs()
                        < 1e-8
                );
                assert_eq!(timeline.shared.undo.borrow().len(), 1);

                let click = (0..row.observe_controllers().n_items())
                    .filter_map(|index| row.observe_controllers().item(index))
                    .filter_map(|item| item.downcast::<gtk4::GestureClick>().ok())
                    .find(|click| click.button() == 1)
                    .unwrap();
                click.emit_by_name::<()>("released", &[&1_i32, &x, &y]);
                assert_eq!(count(&timeline.shared.model.borrow(), text), 1);

                let right = toolbar.last_child().unwrap();
                let undo = right.first_child().unwrap().downcast::<Button>().unwrap();
                let redo = undo.next_sibling().unwrap().downcast::<Button>().unwrap();
                undo.emit_clicked();
                assert_eq!(
                    bounds(&timeline.shared.model.borrow(), text, 0),
                    Some((1.0, 2.0))
                );
                redo.emit_clicked();
                assert!(
                    (bounds(&timeline.shared.model.borrow(), text, 0).unwrap().1 - 3.0).abs()
                        < 1e-8
                );

                let hooks = Hooks {
                    shared: timeline.shared.clone(),
                    redraw: timeline.redraw.clone(),
                    pause: timeline.pause.clone(),
                    start_playback: timeline.start_playback.clone(),
                    dragged: Rc::new(Cell::new(false)),
                };
                let menu = build_menu(text, 0, 1.0, &hooks);
                assert!(menu.has_css_class("settings-select-popover"));
                let rows = menu.child().unwrap();
                let duplicate = rows.first_child().unwrap().downcast::<Button>().unwrap();
                assert!(duplicate.is_sensitive());
                assert!(duplicate.has_css_class("history-action-btn"));
                duplicate.emit_clicked();
                assert_eq!(count(&timeline.shared.model.borrow(), text), 2);
                undo.emit_clicked();
                assert_eq!(count(&timeline.shared.model.borrow(), text), 1);
                let menu = build_menu(text, 0, 1.0, &hooks);
                let rows = menu.child().unwrap();
                let hide = rows
                    .first_child()
                    .unwrap()
                    .next_sibling()
                    .unwrap()
                    .downcast::<Button>()
                    .unwrap();
                hide.emit_clicked();
                let model = timeline.shared.model.borrow();
                assert!(if text {
                    model.text_segments[0].is_disabled
                } else {
                    model.segments[0].is_disabled
                });
                drop(model);
                let menu = build_menu(text, 0, 1.0, &hooks);
                let rows = menu.child().unwrap();
                let show = rows
                    .first_child()
                    .unwrap()
                    .next_sibling()
                    .unwrap()
                    .downcast::<Button>()
                    .unwrap();
                let label = show
                    .child()
                    .unwrap()
                    .last_child()
                    .unwrap()
                    .downcast::<Label>()
                    .unwrap();
                assert_eq!(label.text(), t("Show"));
                let delete = rows.last_child().unwrap().downcast::<Button>().unwrap();
                assert!(delete.has_css_class("history-action-btn"));
                delete.emit_clicked();
                assert_eq!(count(&timeline.shared.model.borrow(), text), 0);
                undo.emit_clicked();
                assert_eq!(count(&timeline.shared.model.borrow(), text), 1);
            }
        }) else {
            eprintln!("skipping: GTK unavailable");
            return;
        };
    }

    #[test]
    fn duplicate_menu_is_disabled_when_no_gap_is_available() {
        let Some(()) = crate::test_support::with_gtk(|| {
            for text in [false, true] {
                let timeline = super::super::build(1.0, true);
                add(&mut timeline.shared.model.borrow_mut(), text, 5.0);
                let hooks = Hooks {
                    shared: timeline.shared.clone(),
                    redraw: timeline.redraw.clone(),
                    pause: timeline.pause.clone(),
                    start_playback: timeline.start_playback.clone(),
                    dragged: Rc::new(Cell::new(false)),
                };
                let menu = build_menu(text, 0, 1.0, &hooks);
                let duplicate = menu
                    .child()
                    .unwrap()
                    .first_child()
                    .unwrap()
                    .downcast::<Button>()
                    .unwrap();
                assert!(!duplicate.is_sensitive());
            }
        }) else {
            eprintln!("skipping: GTK unavailable");
            return;
        };
    }

    #[test]
    fn clip_rows_use_the_same_computed_style_as_settings_options() {
        let Some(()) = crate::test_support::with_gtk(|| {
            crate::settings::install_window_css();
            crate::settings::shell::install_select_menu_css();
            for light in [false, true] {
                let root = GtkBox::new(Orientation::Vertical, 0);
                root.add_css_class("editor-root");
                root.add_css_class(if light {
                    "editor-theme-light"
                } else {
                    "editor-theme-dark"
                });
                let settings = crate::settings::select::SettingsSelect::new([("a", "Option")], "a");
                root.append(settings.widget());
                let child = settings.widget().first_child().unwrap();
                let settings_popover = child.next_sibling().unwrap().downcast::<Popover>().unwrap();
                let settings_row = settings_popover
                    .child()
                    .unwrap()
                    .first_child()
                    .unwrap()
                    .downcast::<Button>()
                    .unwrap();
                let timeline = super::super::build(1.0, !light);
                add(&mut timeline.shared.model.borrow_mut(), false, 1.0);
                let hooks = Hooks {
                    shared: timeline.shared.clone(),
                    redraw: timeline.redraw.clone(),
                    pause: timeline.pause.clone(),
                    start_playback: timeline.start_playback.clone(),
                    dragged: Rc::new(Cell::new(false)),
                };
                let menu = build_menu(false, 0, 1.0, &hooks);
                menu.set_parent(&root);
                let clip_row = menu
                    .child()
                    .unwrap()
                    .first_child()
                    .unwrap()
                    .downcast::<Button>()
                    .unwrap();
                assert_eq!(
                    clip_row.measure(Orientation::Vertical, -1).0,
                    settings_row.measure(Orientation::Vertical, -1).0
                );
                assert_eq!(
                    clip_row.style_context().color(),
                    settings_row.style_context().color()
                );
                clip_row.set_sensitive(false);
                settings_row.set_sensitive(false);
                assert_eq!(
                    clip_row.style_context().color(),
                    settings_row.style_context().color()
                );
                menu.unparent();
                settings_popover.unparent();
            }
        }) else {
            eprintln!("skipping: GTK unavailable");
            return;
        };
    }

    #[test]
    fn changing_source_invalidates_open_menus_and_active_drags() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let timeline = super::super::build(1.0, true);
            add(&mut timeline.shared.model.borrow_mut(), false, 1.0);
            timeline.dock.measure(Orientation::Horizontal, -1);
            let height = timeline.dock.measure(Orientation::Vertical, 600).0.max(320);
            timeline.dock.allocate(600, height, -1, None);
            let card = timeline.dock.first_child().unwrap();
            let board = card.last_child().unwrap().downcast::<Overlay>().unwrap();
            let tracks = board.child().unwrap();
            let row = tracks
                .first_child()
                .unwrap()
                .next_sibling()
                .unwrap()
                .next_sibling()
                .unwrap();
            let width = f64::from(row.width());
            let y = f64::from(row.height()) / 2.0;
            let drag = (0..row.observe_controllers().n_items())
                .filter_map(|index| row.observe_controllers().item(index))
                .find_map(|item| item.downcast::<gtk4::GestureDrag>().ok())
                .unwrap();
            let hooks = Hooks {
                shared: timeline.shared.clone(),
                redraw: timeline.redraw.clone(),
                pause: timeline.pause.clone(),
                start_playback: timeline.start_playback.clone(),
                dragged: Rc::new(Cell::new(false)),
            };
            let menu = build_menu(false, 0, 1.0, &hooks);
            let hide = menu
                .child()
                .unwrap()
                .first_child()
                .unwrap()
                .next_sibling()
                .unwrap()
                .downcast::<Button>()
                .unwrap();
            drag.emit_by_name::<()>("drag-begin", &[&(width / 3.0), &y]);
            drag.emit_by_name::<()>("drag-update", &[&(width / 6.0), &0.0_f64]);
            timeline.reset_source();
            add(&mut timeline.shared.model.borrow_mut(), false, 1.0);
            drag.emit_by_name::<()>("drag-update", &[&(width / 3.0), &0.0_f64]);
            drag.emit_by_name::<()>("drag-end", &[&(width / 3.0), &0.0_f64]);
            hide.emit_clicked();
            let model = timeline.shared.model.borrow();
            assert_eq!(bounds(&model, false, 0), Some((1.0, 2.0)));
            assert!(!model.segments[0].is_disabled);
            assert!(timeline.shared.undo.borrow().is_empty());
        }) else {
            eprintln!("skipping: GTK unavailable");
            return;
        };
    }
}
