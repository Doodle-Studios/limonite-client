//! Keyboard and mouse input, through a keymap (PLAN.md, "Input"): code asks
//! about actions, never keys, so every binding can change without touching
//! it. Saving and rebinding come with M5's settings.

use std::collections::HashSet;

use winit::{event::ElementState, keyboard::KeyCode};

use crate::render::camera::Movement;

/// Something the player can do.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    Forward,
    Back,
    Left,
    Right,
    Up,
    Down,
    /// Held: fly faster.
    Faster,
    /// Lets go of the mouse cursor.
    ReleaseCursor,
    /// Logs the pose as a `/tp` command for the official client.
    LogPose,
}

/// Which key does what.
pub struct Keymap {
    bindings: Vec<(KeyCode, Action)>,
}

impl Default for Keymap {
    fn default() -> Self {
        Self {
            bindings: vec![
                (KeyCode::KeyW, Action::Forward),
                (KeyCode::KeyS, Action::Back),
                (KeyCode::KeyA, Action::Left),
                (KeyCode::KeyD, Action::Right),
                (KeyCode::Space, Action::Up),
                (KeyCode::ShiftLeft, Action::Down),
                (KeyCode::ControlLeft, Action::Faster),
                (KeyCode::Escape, Action::ReleaseCursor),
                (KeyCode::KeyP, Action::LogPose),
            ],
        }
    }
}

impl Keymap {
    /// The actions bound to a key.
    pub fn actions(&self, key: KeyCode) -> impl Iterator<Item = Action> + '_ {
        self.bindings
            .iter()
            .filter(move |(bound, _)| *bound == key)
            .map(|&(_, action)| action)
    }
}

/// The state of every action, and the mouse motion since it was last read.
#[derive(Default)]
pub struct Input {
    keymap: Keymap,
    held: HashSet<Action>,
    /// Actions pressed since [`Input::take_pressed`] was last called.
    pressed: Vec<Action>,
    /// Raw mouse motion since [`Input::take_mouse`], in counts.
    mouse: (f64, f64),
}

impl Input {
    /// A key changed. Auto-repeat presses arrive as more presses, and are
    /// ignored: an action is pressed once and held until released.
    pub fn key(&mut self, key: KeyCode, state: ElementState) {
        let actions: Vec<Action> = self.keymap.actions(key).collect();
        for action in actions {
            match state {
                ElementState::Pressed => {
                    if self.held.insert(action) {
                        self.pressed.push(action);
                    }
                }
                ElementState::Released => {
                    self.held.remove(&action);
                }
            }
        }
    }

    pub fn mouse_motion(&mut self, dx: f64, dy: f64) {
        self.mouse.0 += dx;
        self.mouse.1 += dy;
    }

    #[must_use]
    pub fn held(&self, action: Action) -> bool {
        self.held.contains(&action)
    }

    pub fn take_pressed(&mut self) -> Vec<Action> {
        std::mem::take(&mut self.pressed)
    }

    pub fn take_mouse(&mut self) -> (f64, f64) {
        std::mem::take(&mut self.mouse)
    }

    /// Lets go of everything, e.g. when the window loses focus and stops
    /// hearing about key releases.
    pub fn clear(&mut self) {
        self.held.clear();
        self.pressed.clear();
        self.mouse = (0.0, 0.0);
    }

    /// The camera's movement from the held actions.
    #[must_use]
    pub fn movement(&self) -> Movement {
        let axis = |plus, minus| f64::from(i8::from(self.held(plus)) - i8::from(self.held(minus)));
        Movement {
            forward: axis(Action::Forward, Action::Back),
            right: axis(Action::Right, Action::Left),
            up: axis(Action::Up, Action::Down),
            fast: self.held(Action::Faster),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_drive_actions_through_the_keymap() {
        let mut input = Input::default();
        input.key(KeyCode::KeyW, ElementState::Pressed);
        input.key(KeyCode::KeyW, ElementState::Pressed); // auto-repeat
        input.key(KeyCode::KeyA, ElementState::Pressed);
        input.key(KeyCode::ControlLeft, ElementState::Pressed);
        assert_eq!(
            input.movement(),
            Movement {
                forward: 1.0,
                right: -1.0,
                up: 0.0,
                fast: true
            }
        );
        assert_eq!(input.take_pressed(), [Action::Forward, Action::Left, Action::Faster]);
        assert!(input.take_pressed().is_empty());

        input.key(KeyCode::KeyW, ElementState::Released);
        input.key(KeyCode::KeyS, ElementState::Pressed);
        assert!((input.movement().forward + 1.0).abs() < f64::EPSILON);

        input.mouse_motion(3.0, -1.0);
        input.mouse_motion(1.0, 0.5);
        assert_eq!(input.take_mouse(), (4.0, -0.5));
        assert_eq!(input.take_mouse(), (0.0, 0.0));

        input.clear();
        assert_eq!(input.movement(), Movement::default());
        // An unbound key does nothing.
        input.key(KeyCode::KeyZ, ElementState::Pressed);
        assert!(input.take_pressed().is_empty());
    }
}
