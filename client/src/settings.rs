//! What the player chooses. M5 saves these to a settings file; in M4 they
//! come from flags.

use pumpkin_util::GameMode;

/// The player's settings.
#[derive(Clone, Copy, Debug)]
pub struct Settings {
    /// The gamemode the embedded server gives a host it hasn't seen before.
    ///
    /// Creative by default, because M4's fly camera passes through terrain,
    /// and a survival host with its head in a block suffocates (m4-plan
    /// Finding 11). M5 revisits the default together with collision.
    ///
    /// A host the world has saved keeps its own mode: the server reads it
    /// from the player's data on join.
    pub gamemode: GameMode,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            gamemode: GameMode::Creative,
        }
    }
}
