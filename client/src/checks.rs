//! Milestone 2's acceptance checks, each reported as one `PASS` or `FAIL` line.
//!
//! The fourth check, "no socket is opened", can't be made from inside the
//! process; it's done from outside while the session lingers.

use pumpkin_core::server::Server;
use tracing::{error, info};

use crate::session::Spawn;

/// How far east the host must have moved, as the server sees it.
pub const MIN_MOVE: f64 = 4.0;

pub struct Check {
    name: &'static str,
    passed: bool,
    detail: String,
}

impl Check {
    pub const fn new(name: &'static str, passed: bool, detail: String) -> Self {
        Self {
            name,
            passed,
            detail,
        }
    }

    pub const fn passed(&self) -> bool {
        self.passed
    }
}

/// Whether the server's copy of the host has moved at least [`MIN_MOVE`]
/// blocks east of where `StartGame` put it.
///
/// This reads the server's own state, not an echo: the position went from
/// our `PlayerAuthInput` through the `LocalChannel`, the server's read loop,
/// its packet queue and its tick thread, which applied it.
pub fn movement(server: &Server, spawn: &Spawn) -> Check {
    let players = server.get_all_players();
    let Some(player) = players.first() else {
        return Check::new("movement", false, "server has no player".to_string());
    };
    let entity = &player.living_entity.entity;
    // `StartGame` gives the eye position; the server stores the feet.
    let eye_height = f64::from(entity.entity_type.eye_height);
    let start_x = f64::from(spawn.eye_position.x);
    let start_feet_y = f64::from(spawn.eye_position.y) - eye_height;
    let now = player.position();
    let moved = now.x - start_x;
    Check::new(
        "movement",
        moved >= MIN_MOVE,
        format!(
            "the server's player moved {moved:.2} blocks east (needs {MIN_MOVE}); \
             feet y {start_feet_y:.2} -> {:.2}",
            now.y
        ),
    )
}

/// Logs one line per check and returns whether they all passed.
pub fn report(checks: &[Check]) -> bool {
    for check in checks {
        if check.passed {
            info!("PASS {}: {}", check.name, check.detail);
        } else {
            error!("FAIL {}: {}", check.name, check.detail);
        }
    }
    checks.iter().all(|check| check.passed)
}
