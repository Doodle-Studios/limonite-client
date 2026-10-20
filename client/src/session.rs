//! The host's session with the embedded server.
//!
//! A 20 Hz loop on the main thread, the skeleton of the future render loop:
//! each tick drains what the server sent, reacts to it, and sends this tick's
//! input. Everything goes through a `LocalClientEnd`, whose methods never
//! wait, so the loop needs no async runtime.

use std::{
    fmt, thread,
    time::{Duration, Instant},
};

use bytes::Bytes;
use pumpkin_core::net::bedrock::{
    level_chunk::CLevelChunk,
    local::{LocalClientEnd, LocalConnector},
};
use pumpkin_protocol::{
    BClientPacket, RawPacket,
    bedrock::{
        client::{
            CChunkRadiusUpdated, CDisconnect, CPlayStatus, CResourcePackStackPacket,
            CResourcePacksInfo, CStartGame,
        },
        server::{
            player_auth_input::{InputData, SPlayerAuthInput},
            request_chunk_radius::SRequestChunkRadius,
            resource_pack_client_response::SResourcePackClientResponse,
            set_local_player_as_initialized::SSetLocalPlayerAsInitialized,
        },
    },
    codec::{bitset::Bitset, var_int::VarInt, var_uint::VarUInt, var_ulong::VarULong},
    packet::Packet,
    serial::PacketRead,
};
use pumpkin_util::math::{vector2::Vector2, vector3::Vector3};
use tokio::sync::mpsc::error::{TryRecvError, TrySendError};
use tracing::{debug, info, warn};

use crate::{login, packets};

/// One client tick, as in vanilla.
pub const TICK: Duration = Duration::from_millis(50);
/// The view distance the host asks for, in chunks.
const CHUNK_RADIUS: i32 = 8;
/// Bedrock's input-mode and play-mode values for mouse and keyboard on a
/// screen, as the official client sends them.
const INPUT_MODE_MOUSE: u32 = 1;
const PLAY_MODE_SCREEN: u32 = 2;
/// The `delta.y` the official client reports every tick while standing on the
/// ground: one tick of gravity, cancelled by the collision.
const STANDING_DELTA_Y: f32 = -0.078_4;

/// How far the session has got. Each stage names what it's waiting for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stage {
    /// `SLogin` sent; waiting for `ResourcePacksInfo`.
    LoggingIn,
    /// `HAVE_ALL_PACKS` sent; waiting for `ResourcePackStack`.
    ResourcePacks,
    /// `COMPLETED` sent; waiting for `StartGame`.
    StartingGame,
    /// `StartGame` received; waiting for `PlayStatus::PlayerSpawn`.
    Spawning,
    /// In the world.
    Spawned,
    /// The server ended the connection.
    Disconnected,
}

/// What `StartGame` told us about the player.
#[derive(Clone, Copy, Debug)]
pub struct Spawn {
    pub runtime_id: u64,
    /// Where the player's eyes are; Bedrock positions are at eye height.
    pub eye_position: Vector3<f32>,
    pub pitch: f32,
    pub yaw: f32,
}

/// The session timed out before reaching the stage it waited for.
#[derive(Debug)]
pub struct TimedOut {
    pub stage: Stage,
}

impl fmt::Display for TimedOut {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "timed out while in stage {:?}", self.stage)
    }
}

pub struct Session {
    client: LocalClientEnd,
    stage: Stage,
    spawn: Option<Spawn>,
    /// The eye position the host reports each tick.
    position: Vector3<f32>,
    /// Ticks of input sent so far; also the input packets' tick number.
    input_tick: u64,
    level_chunks: u32,
}

impl Session {
    /// Connects to the embedded server and sends the login.
    ///
    /// # Errors
    /// If the server isn't accepting local connections.
    pub fn connect(connector: &LocalConnector, name: &str) -> std::io::Result<Self> {
        let client = connector.connect()?;
        let session = Self {
            client,
            stage: Stage::LoggingIn,
            spawn: None,
            position: Vector3::new(0.0, 0.0, 0.0),
            input_tick: 0,
            level_chunks: 0,
        };
        // No `RequestNetworkSettings` first: compression then never turns on,
        // and every batch stays plain bytes (recon §5).
        session.send(&login::host_login(name));
        info!("Logging in as {name}");
        Ok(session)
    }

    pub const fn stage(&self) -> Stage {
        self.stage
    }

    pub const fn level_chunks(&self) -> u32 {
        self.level_chunks
    }

    /// Runs ticks until `done` returns true or `deadline` passes.
    ///
    /// # Errors
    /// [`TimedOut`] with the stage the session was stuck in.
    pub fn run_until(
        &mut self,
        deadline: Instant,
        mut done: impl FnMut(&Self) -> bool,
    ) -> Result<(), TimedOut> {
        let mut next_tick = Instant::now();
        loop {
            self.tick();
            if done(self) {
                return Ok(());
            }
            if self.stage == Stage::Disconnected || Instant::now() >= deadline {
                return Err(TimedOut { stage: self.stage });
            }
            next_tick += TICK;
            if let Some(wait) = next_tick.checked_duration_since(Instant::now()) {
                thread::sleep(wait);
            }
        }
    }

    /// One tick: handle everything that arrived, then send this tick's input.
    fn tick(&mut self) {
        loop {
            match self.client.try_recv() {
                Ok(batch) => self.handle_batch(batch),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    if self.stage != Stage::Disconnected {
                        warn!("The server closed the connection");
                        self.stage = Stage::Disconnected;
                    }
                    return;
                }
            }
        }

        // Input starts with `StartGame`: the server ignores it until the
        // player has loaded, but any packet keeps its 10 s timeout away.
        if self.spawn.is_some() && self.stage != Stage::Disconnected {
            let input = self.input();
            self.send(&input);
            self.input_tick += 1;
        }
    }

    fn handle_batch(&mut self, batch: Bytes) {
        match packets::split_batch(batch) {
            Ok(packets) => {
                for packet in packets {
                    self.handle_packet(&packet);
                }
            }
            Err(err) => warn!("Couldn't split a batch from the server: {err}"),
        }
    }

    fn handle_packet(&mut self, packet: &RawPacket) {
        debug!("<- {}", packets::PacketName(packet.id));
        let mut body = packet.payload.as_ref();
        match packet.id {
            CPlayStatus::PACKET_ID => match CPlayStatus::read(&mut body) {
                Ok(CPlayStatus::LoginSuccess) => info!("Login accepted"),
                Ok(CPlayStatus::PlayerSpawn) => self.on_player_spawn(),
                Ok(status) => warn!("PlayStatus {}", status as i32),
                Err(err) => warn!("Bad PlayStatus: {err}"),
            },
            CResourcePacksInfo::PACKET_ID => {
                self.send_pack_response(SResourcePackClientResponse::STATUS_HAVE_ALL_PACKS);
                self.stage = Stage::ResourcePacks;
            }
            CResourcePackStackPacket::PACKET_ID => {
                self.send_pack_response(SResourcePackClientResponse::STATUS_COMPLETED);
                self.stage = Stage::StartingGame;
            }
            CStartGame::PACKET_ID => match CStartGame::read(&mut body) {
                Ok(start_game) => self.on_start_game(&start_game),
                Err(err) => warn!("Bad StartGame: {err}"),
            },
            CChunkRadiusUpdated::PACKET_ID => match CChunkRadiusUpdated::read(&mut body) {
                Ok(update) => info!("Chunk radius is {}", update.chunk_radius.0),
                Err(err) => warn!("Bad ChunkRadiusUpdated: {err}"),
            },
            CLevelChunk::PACKET_ID => self.level_chunks += 1,
            CDisconnect::PACKET_ID => {
                match CDisconnect::read(&mut body) {
                    Ok(disconnect) => warn!(
                        "Disconnected by the server (reason {}): {}",
                        disconnect.reason.0, disconnect.message
                    ),
                    Err(err) => warn!("Disconnected by the server (unreadable: {err})"),
                }
                self.stage = Stage::Disconnected;
            }
            _ => {}
        }
    }

    fn on_start_game(&mut self, start_game: &CStartGame) {
        let spawn = Spawn {
            runtime_id: start_game.runtime_entity_id.0,
            eye_position: start_game.position,
            pitch: start_game.pitch,
            yaw: start_game.yaw,
        };
        let settings = &start_game.level_settings;
        let block = settings.spawn_position.0;
        info!(
            "StartGame: runtime id {}, seed {}, world spawn ({}, {}, {}), player eyes at ({:.2}, {:.2}, {:.2})",
            spawn.runtime_id,
            settings.seed,
            block.x,
            block.y,
            block.z,
            spawn.eye_position.x,
            spawn.eye_position.y,
            spawn.eye_position.z,
        );
        self.position = spawn.eye_position;
        self.spawn = Some(spawn);
        self.stage = Stage::Spawning;
        self.send(&SRequestChunkRadius {
            chunk_radius: VarInt(CHUNK_RADIUS),
            max_chunk_radius: CHUNK_RADIUS as u8,
        });
    }

    fn on_player_spawn(&mut self) {
        let Some(spawn) = self.spawn else {
            warn!("PlayerSpawn arrived before StartGame");
            return;
        };
        self.send(&SSetLocalPlayerAsInitialized {
            player_id: VarULong(spawn.runtime_id),
        });
        self.stage = Stage::Spawned;
        info!("Spawned after {} LevelChunk packets", self.level_chunks);
    }

    fn send_pack_response(&self, response: u8) {
        self.send(&SResourcePackClientResponse {
            response,
            download_size: 0,
            pack_ids: Vec::new(),
        });
    }

    /// This tick's `PlayerAuthInput`: standing still on the ground.
    fn input(&self) -> SPlayerAuthInput {
        let (pitch, yaw) = self
            .spawn
            .map_or((0.0, 0.0), |spawn| (spawn.pitch, spawn.yaw));
        // The flags the official client sets while standing on the ground.
        let mut input_data = Bitset::<66>::default();
        input_data.set(InputData::BlockBreakingDelayEnabled as usize, true);
        input_data.set(InputData::VerticalCollision as usize, true);
        SPlayerAuthInput {
            pitch,
            yaw,
            position: self.position,
            move_vec: Vector2::new(0.0, 0.0),
            head_yaw: yaw,
            input_data,
            input_mode: VarUInt(INPUT_MODE_MOUSE),
            play_mode: VarUInt(PLAY_MODE_SCREEN),
            interaction_model: VarInt(0),
            interact_pitch: pitch,
            interact_yaw: yaw,
            tick: VarULong(self.input_tick),
            delta: Vector3::new(0.0, STANDING_DELTA_Y, 0.0),
            block_actions: None,
            item_interaction: None,
            item_stack_request: None,
            vehicle_rotation: None,
            vehicle_unique_id: None,
            analog_move: Vector2::new(0.0, 0.0),
            camera_orientation: look_direction(pitch, yaw),
            raw_move: Vector2::new(0.0, 0.0),
        }
    }

    /// Frames and queues one packet for the server.
    fn send<P: BClientPacket>(&self, packet: &P) {
        let batch = match packets::frame(packet) {
            Ok(batch) => batch,
            Err(err) => {
                warn!("Couldn't encode packet {}: {err}", P::PACKET_ID);
                return;
            }
        };
        match self.client.try_send(batch) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                warn!(
                    "The server's queue is full; dropped packet {}",
                    P::PACKET_ID
                );
            }
            Err(TrySendError::Closed(_)) => {
                debug!("Not sending packet {}: connection closed", P::PACKET_ID);
            }
        }
    }
}

/// The unit vector the camera looks along, from Minecraft angles in degrees:
/// yaw 0 faces +Z, yaw -90 faces +X, and positive pitch looks down.
fn look_direction(pitch: f32, yaw: f32) -> Vector3<f32> {
    let (pitch, yaw) = (pitch.to_radians(), yaw.to_radians());
    Vector3::new(
        -yaw.sin() * pitch.cos(),
        -pitch.sin(),
        yaw.cos() * pitch.cos(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The official client's own `camera_orientation` for its pitch and yaw,
    /// from the M2 golden fixtures.
    #[test]
    fn look_direction_matches_the_official_client() {
        let fixtures: [&[u8]; 3] = [
            include_bytes!(
                "../../pumpkin/crates/pumpkin-protocol/src/bedrock/client_codecs/fixtures/player_auth_input-idle.bin"
            ),
            include_bytes!(
                "../../pumpkin/crates/pumpkin-protocol/src/bedrock/client_codecs/fixtures/player_auth_input-walking.bin"
            ),
            include_bytes!(
                "../../pumpkin/crates/pumpkin-protocol/src/bedrock/client_codecs/fixtures/player_auth_input-jumping.bin"
            ),
        ];
        for mut fixture in fixtures {
            let input = SPlayerAuthInput::read(&mut fixture).unwrap();
            let ours = look_direction(input.pitch, input.yaw);
            let theirs = input.camera_orientation;
            for (a, b) in [(ours.x, theirs.x), (ours.y, theirs.y), (ours.z, theirs.z)] {
                assert!((a - b).abs() < 1e-4, "ours {ours:?}, official {theirs:?}");
            }
        }
    }
}
