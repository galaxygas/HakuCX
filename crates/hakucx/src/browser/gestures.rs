/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Mouse gestures and rocker gestures over pages (Settings → Features → Shortcuts).
//!
//! - Mouse gestures: hold the right button and draw. Left = back, right = forward,
//!   up then down = reload, down then right = close the tab.
//! - Rocker gestures: hold right and click left = back; hold left and click right = forward.
//!
//! While either is on, the right button's press is held back from the page until it is clear
//! what it was: released without moving, the page gets the press and the release (so its
//! context menu still opens); used for a gesture, the page never sees it.

use haku_core::ipc::Command;
use servo::{
    InputEvent, MouseButton as ServoMouseButton, MouseButtonAction, MouseButtonEvent, WebView,
    WebViewPoint,
};
use winit::dpi::PhysicalPosition;
use winit::event::{ElementState, MouseButton};

use super::{Browser, PointerTarget};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Left,
    Right,
    Up,
    Down,
}

/// The strokes of a path: a new stroke starts when the pointer has moved `step` pixels, and
/// repeats of the same direction merge.
pub fn strokes(path: &[(f64, f64)], step: f64) -> Vec<Dir> {
    let mut out = Vec::new();
    let Some(&first) = path.first() else {
        return out;
    };
    let mut anchor = first;
    for &(x, y) in path {
        let (dx, dy) = (x - anchor.0, y - anchor.1);
        if dx.abs().max(dy.abs()) < step {
            continue;
        }
        let dir = if dx.abs() > dy.abs() {
            if dx > 0.0 { Dir::Right } else { Dir::Left }
        } else if dy > 0.0 {
            Dir::Down
        } else {
            Dir::Up
        };
        if out.last() != Some(&dir) {
            out.push(dir);
        }
        anchor = (x, y);
    }
    out
}

/// What a gesture does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gesture {
    Back,
    Forward,
    Reload,
    CloseTab,
}

pub fn recognize(strokes: &[Dir]) -> Option<Gesture> {
    match strokes {
        [Dir::Left] => Some(Gesture::Back),
        [Dir::Right] => Some(Gesture::Forward),
        [Dir::Up, Dir::Down] => Some(Gesture::Reload),
        [Dir::Down, Dir::Right] => Some(Gesture::CloseTab),
        _ => None,
    }
}

/// A right-button press being held back from the page.
pub struct RightHold {
    webview: WebView,
    point: WebViewPoint,
    path: Vec<(f64, f64)>,
}

#[derive(Default)]
pub struct GestureState {
    right: Option<RightHold>,
    /// The left button went down on a page (for "hold left, click right").
    left_on_page: bool,
    /// Releases that belong to a rocker gesture and must not reach the page.
    swallow_left_up: bool,
    swallow_right_up: bool,
}

impl Browser {
    fn gestures_on(&self) -> (bool, bool) {
        let s = self.settings.borrow();
        (s.shortcuts.mouse_gestures, s.shortcuts.rocker_gestures)
    }

    fn run_gesture(&self, gesture: Gesture) {
        let command = match gesture {
            Gesture::Back => Command::Back { tab: None },
            Gesture::Forward => Command::Forward { tab: None },
            Gesture::Reload => Command::Reload {
                tab: None,
                bypass_cache: false,
            },
            Gesture::CloseTab => match self.active_tab_id() {
                Some(tab) => Command::CloseTab { tab },
                None => return,
            },
        };
        self.run_command(command);
    }

    /// Mouse buttons. Returns true when the event was used for a gesture (do not pass it on).
    pub(crate) fn gesture_button(
        &self,
        target: PointerTarget,
        webview: &Option<WebView>,
        point: WebViewPoint,
        state: ElementState,
        button: MouseButton,
    ) -> bool {
        let (gestures, rocker) = self.gestures_on();
        if !gestures && !rocker {
            return false;
        }
        let on_page = matches!(target, PointerTarget::Page(_)) && webview.is_some();
        let pos = self.cursor.get();
        let mut g = self.gesture.borrow_mut();
        match (button, state) {
            (MouseButton::Right, ElementState::Pressed) if on_page => {
                if rocker && g.left_on_page {
                    g.swallow_right_up = true;
                    drop(g);
                    self.run_gesture(Gesture::Forward);
                    return true;
                }
                g.right = Some(RightHold {
                    webview: webview.clone().expect("checked"),
                    point,
                    path: vec![(pos.x, pos.y)],
                });
                true
            }
            (MouseButton::Right, ElementState::Released) => {
                if g.swallow_right_up {
                    g.swallow_right_up = false;
                    g.right = None;
                    return true;
                }
                let Some(hold) = g.right.take() else {
                    return false;
                };
                drop(g);
                let step = 30.0 * self.window.scale_factor();
                let drawn = strokes(&hold.path, step);
                if drawn.is_empty() {
                    // A plain right click: give the page its press and release.
                    for action in [MouseButtonAction::Down, MouseButtonAction::Up] {
                        hold.webview.notify_input_event(InputEvent::MouseButton(
                            MouseButtonEvent::new(action, ServoMouseButton::Secondary, hold.point),
                        ));
                    }
                } else if gestures && let Some(gesture) = recognize(&drawn) {
                    self.run_gesture(gesture);
                }
                true
            }
            (MouseButton::Left, ElementState::Pressed) => {
                if rocker && g.right.is_some() {
                    g.right = None;
                    g.swallow_right_up = true;
                    g.swallow_left_up = true;
                    drop(g);
                    self.run_gesture(Gesture::Back);
                    return true;
                }
                g.left_on_page = on_page;
                false
            }
            (MouseButton::Left, ElementState::Released) => {
                g.left_on_page = false;
                std::mem::take(&mut g.swallow_left_up)
            }
            _ => false,
        }
    }

    /// Pointer movement while the right button is held (the gesture's path).
    pub(crate) fn gesture_move(&self, pos: PhysicalPosition<f64>) {
        if let Some(hold) = self.gesture.borrow_mut().right.as_mut()
            && hold.path.len() < 4096
        {
            hold.path.push((pos.x, pos.y));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(from: (f64, f64), to: (f64, f64)) -> Vec<(f64, f64)> {
        (0..=20)
            .map(|i| {
                let t = i as f64 / 20.0;
                (from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t)
            })
            .collect()
    }

    #[test]
    fn strokes_and_gestures() {
        let left = line((300.0, 300.0), (100.0, 305.0));
        assert_eq!(strokes(&left, 30.0), vec![Dir::Left]);
        assert_eq!(recognize(&strokes(&left, 30.0)), Some(Gesture::Back));

        let mut up_down = line((200.0, 300.0), (205.0, 150.0));
        up_down.extend(line((205.0, 150.0), (200.0, 320.0)));
        assert_eq!(recognize(&strokes(&up_down, 30.0)), Some(Gesture::Reload));

        let mut down_right = line((200.0, 100.0), (200.0, 260.0));
        down_right.extend(line((200.0, 260.0), (380.0, 262.0)));
        assert_eq!(
            recognize(&strokes(&down_right, 30.0)),
            Some(Gesture::CloseTab)
        );

        // A wobble under the step is a plain click, not a gesture.
        let still = line((200.0, 200.0), (210.0, 205.0));
        assert!(strokes(&still, 30.0).is_empty());
        assert_eq!(recognize(&[Dir::Up]), None);
    }
}
