use gtk4::prelude::*;
use gtk4::{gdk, glib, Align, ContentFit, Picture, Stack, StackTransitionType};
use std::cell::Cell;

use super::motion;
use super::OnboardingStep;

const WAVE: &[u8] = include_bytes!("assets/mascot/wave.png");
const CAPTURE: &[u8] = include_bytes!("assets/mascot/capture.png");
const CLOUD: &[u8] = include_bytes!("assets/mascot/cloud.png");
const GUIDE: &[u8] = include_bytes!("assets/mascot/guide.png");
const CELEBRATE: &[u8] = include_bytes!("assets/mascot/celebrate.png");

pub struct Mascot {
    stack: Stack,
    animate_welcome: bool,
}

impl Mascot {
    pub fn new(step: OnboardingStep) -> Self {
        let stack = Stack::new();
        stack.add_css_class("onboarding-mascot");
        stack.set_size_request(96, 96);
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
            OnboardingStep::Welcome => WAVE,
            OnboardingStep::HowToUse => CAPTURE,
            OnboardingStep::Cloud => CLOUD,
            OnboardingStep::ChromeExtension => GUIDE,
            OnboardingStep::Complete => CELEBRATE,
        });
        stack.add_named(&action, Some("action"));
        let animate_welcome = step == OnboardingStep::Welcome;
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
