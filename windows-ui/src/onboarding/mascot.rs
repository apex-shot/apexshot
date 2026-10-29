//! The mascot that heads each step, drawn from the Linux build's artwork.

use gtk4::prelude::*;
use gtk4::{gdk, glib, Align, ContentFit, Picture, Stack, StackTransitionType};
use std::cell::Cell;

use super::motion;
use super::{px, OnboardingStep as Step};

const WAVE: &[u8] = include_bytes!("../../../src/onboarding/assets/mascot/wave.png");
const CAPTURE: &[u8] = include_bytes!("../../../src/onboarding/assets/mascot/capture.png");
const CLOUD: &[u8] = include_bytes!("../../../src/onboarding/assets/mascot/cloud.png");
const CELEBRATE: &[u8] = include_bytes!("../../../src/onboarding/assets/mascot/celebrate.png");

pub struct Mascot {
    stack: Stack,
    animate_welcome: bool,
}

impl Mascot {
    pub fn new(step: Step, scale: f64) -> Self {
        let stack = Stack::new();
        stack.add_css_class("onboarding-mascot");
        stack.set_size_request(px(scale, 96.0), px(scale, 96.0));
        stack.set_halign(Align::Center);
        stack.set_transition_type(if motion::enabled() {
            StackTransitionType::Crossfade
        } else {
            StackTransitionType::None
        });
        stack.set_transition_duration(260);
        stack.set_hhomogeneous(false);
        stack.set_vhomogeneous(false);

        let wave = picture(WAVE);
        stack.add_named(&wave, Some("wave"));
        let action = picture(match step {
            Step::Welcome => WAVE,
            Step::HowToUse => CAPTURE,
            Step::Cloud => CLOUD,
            Step::Complete => CELEBRATE,
        });
        stack.add_named(&action, Some("action"));
        let animate_welcome = step == Step::Welcome;
        stack.set_visible_child_name(if animate_welcome { "wave" } else { "action" });

        Self {
            stack,
            animate_welcome,
        }
    }

    pub fn widget(&self) -> &Stack {
        &self.stack
    }

    pub fn animate(&self) -> Option<gtk4::TickCallbackId> {
        if !self.animate_welcome {
            return None;
        }
        if !motion::enabled() {
            self.stack.set_visible_child_name("action");
            return None;
        }

        let started = Cell::new(None::<i64>);
        let switched = Cell::new(false);
        Some(self.stack.add_tick_callback(move |widget, clock| {
            if !widget.is_mapped() {
                return glib::ControlFlow::Break;
            }
            let now = clock.frame_time();
            let start = started.get().unwrap_or(now);
            started.set(Some(start));
            let elapsed = now - start;
            if elapsed >= 180_000 && !switched.replace(true) {
                widget.set_visible_child_name("action");
            }
            let rise = motion::ease_out_cubic(elapsed as f64 / 460_000.0);
            widget.set_margin_top((8.0 * (1.0 - rise)).round() as i32);
            widget.set_opacity(motion::ease_out_cubic(elapsed as f64 / 120_000.0));
            if elapsed >= 520_000 {
                widget.set_margin_top(0);
                widget.set_opacity(1.0);
                glib::ControlFlow::Break
            } else {
                glib::ControlFlow::Continue
            }
        }))
    }
}

fn picture(bytes: &'static [u8]) -> Picture {
    let picture = Picture::new();
    picture.set_can_shrink(true);
    picture.set_content_fit(ContentFit::Contain);
    let texture = gdk::Texture::from_bytes(&glib::Bytes::from_static(bytes))
        .expect("embedded mascot images must decode");
    picture.set_paintable(Some(&texture));
    picture.set_hexpand(true);
    picture.set_vexpand(true);
    picture
}
