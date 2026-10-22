//! The fly camera: where the eye is, where it looks, and the matrices that
//! put the world on screen.
//!
//! **Coordinates** (worth teaching). The world is right-handed with y up: +x
//! is east, +y up, +z south. The camera uses Bedrock's angles, in degrees:
//! yaw 0 faces +z, −90 faces +x, and positive pitch looks down. The host
//! sends the same angles to the server, and [`look_direction`] is tested
//! against the official client's own vector, so what the server sees and
//! what we draw can't disagree.
//!
//! **Camera-relative rendering** (worth teaching). Positions are `f64` here.
//! An `f32` has 24 bits of precision, so 30,000 blocks from the origin it
//! can't tell apart points much closer than a millimetre, and the picture
//! jitters. So the view matrix only rotates; each draw is moved by its
//! sub-chunk's origin minus the camera, worked out in `f64` and only then
//! made `f32`, which is small and precise near the camera.

use glam::{DVec3, Mat4, Vec3, Vec4, camera};

/// How fast the camera flies, in blocks a second: creative flying speed.
pub const FLY_SPEED: f64 = 10.92;
/// How much faster it flies with the "faster" key held.
pub const FAST: f64 = 3.0;
/// Degrees of turn per count of raw mouse motion.
const MOUSE_DEGREES: f32 = 0.15;
/// Pitch stops short of straight up or down, where "forward" and "up" would
/// be the same direction and the view matrix would be undefined.
const MAX_PITCH: f32 = 89.9;
/// The vertical field of view, in degrees.
#[cfg_attr(not(test), expect(dead_code, reason = "the terrain draws with it (M4 Step 10)"))]
const FOV_Y: f32 = 70.0;
/// The near and far planes, in blocks.
#[cfg_attr(not(test), expect(dead_code, reason = "the terrain draws with it (M4 Step 10)"))]
const NEAR: f32 = 0.05;
#[cfg_attr(not(test), expect(dead_code, reason = "the terrain draws with it (M4 Step 10)"))]
const FAR: f32 = 2048.0;

/// The unit vector the camera looks along, from Bedrock angles in degrees.
#[must_use]
pub fn look_direction(pitch: f32, yaw: f32) -> Vec3 {
    let (pitch, yaw) = (pitch.to_radians(), yaw.to_radians());
    Vec3::new(-yaw.sin() * pitch.cos(), -pitch.sin(), yaw.cos() * pitch.cos())
}

/// Which way the camera wants to move this frame, each axis from -1 to 1.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Movement {
    /// Forward (+) and back, along the ground.
    pub forward: f64,
    /// Right (+) and left.
    pub right: f64,
    /// Up (+) and down.
    pub up: f64,
    pub fast: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct Camera {
    /// The eye, in world blocks.
    pub position: DVec3,
    pub pitch: f32,
    pub yaw: f32,
    /// Width over height of the window.
    pub aspect: f32,
}

impl Camera {
    #[must_use]
    pub const fn new(position: DVec3, pitch: f32, yaw: f32) -> Self {
        Self {
            position,
            pitch,
            yaw,
            aspect: 16.0 / 9.0,
        }
    }

    #[cfg_attr(not(test), expect(dead_code, reason = "the terrain draws with it (M4 Step 10)"))]
    #[must_use]
    pub fn forward(&self) -> Vec3 {
        look_direction(self.pitch, self.yaw)
    }

    /// Turns by raw mouse motion: right turns right, down looks down.
    pub fn turn(&mut self, dx: f64, dy: f64) {
        self.yaw = (self.yaw + dx as f32 * MOUSE_DEGREES).rem_euclid(360.0);
        if self.yaw > 180.0 {
            self.yaw -= 360.0;
        }
        self.pitch = (self.pitch + dy as f32 * MOUSE_DEGREES).clamp(-MAX_PITCH, MAX_PITCH);
    }

    /// Moves for `seconds` of flight. Forward and right follow the yaw only,
    /// so looking down doesn't fly into the ground.
    pub fn fly(&mut self, movement: Movement, seconds: f64) {
        let yaw = f64::from(self.yaw).to_radians();
        let forward = DVec3::new(-yaw.sin(), 0.0, yaw.cos());
        // Right of forward, with y up: forward × up.
        let right = forward.cross(DVec3::Y);
        let direction = forward * movement.forward + right * movement.right + DVec3::Y * movement.up;
        if direction.length_squared() > 0.0 {
            let speed = FLY_SPEED * if movement.fast { FAST } else { 1.0 };
            self.position += direction.normalize() * speed * seconds;
        }
    }

    /// Rotation only: the camera stays at the origin (camera-relative
    /// rendering, above).
    #[cfg_attr(not(test), expect(dead_code, reason = "the terrain draws with it (M4 Step 10)"))]
    #[must_use]
    pub fn view(&self) -> Mat4 {
        camera::rh::view::look_to_mat4(Vec3::ZERO, self.forward(), Vec3::Y)
    }

    /// Perspective, right-handed, with depth from 0 at the near plane to 1 at
    /// the far plane and y up: WebGPU's convention, which glam files under
    /// `directx`.
    #[cfg_attr(not(test), expect(dead_code, reason = "the terrain draws with it (M4 Step 10)"))]
    #[must_use]
    pub fn projection(&self) -> Mat4 {
        camera::rh::proj::directx::perspective(FOV_Y.to_radians(), self.aspect, NEAR, FAR)
    }

    #[cfg_attr(not(test), expect(dead_code, reason = "the terrain draws with it (M4 Step 10)"))]
    #[must_use]
    pub fn view_projection(&self) -> Mat4 {
        self.projection() * self.view()
    }

    /// The view volume's six planes, camera-relative.
    #[cfg_attr(not(test), expect(dead_code, reason = "the terrain draws with it (M4 Step 10)"))]
    #[must_use]
    pub fn frustum(&self) -> Frustum {
        Frustum::from_matrix(self.view_projection())
    }
}

/// Six planes bounding what the camera sees. A point `p` is inside a plane
/// when `plane.xyz · p + plane.w >= 0`.
#[cfg_attr(not(test), expect(dead_code, reason = "the terrain draws with it (M4 Step 10)"))]
#[derive(Clone, Copy, Debug)]
pub struct Frustum {
    planes: [Vec4; 6],
}

#[cfg_attr(not(test), expect(dead_code, reason = "the terrain draws with it (M4 Step 10)"))]
impl Frustum {
    /// Reads the planes out of a view-projection matrix (Gribb and
    /// Hartmann's method). A clip-space point is visible when
    /// -w ≤ x ≤ w, -w ≤ y ≤ w and 0 ≤ z ≤ w: each inequality is a plane, a
    /// sum or difference of the matrix's rows.
    #[must_use]
    pub fn from_matrix(m: Mat4) -> Self {
        let rows = [m.row(0), m.row(1), m.row(2), m.row(3)];
        let planes = [
            rows[3] + rows[0], // left
            rows[3] - rows[0], // right
            rows[3] + rows[1], // bottom
            rows[3] - rows[1], // top
            rows[2],           // near: z ≥ 0
            rows[3] - rows[2], // far: z ≤ w
        ];
        Self { planes }
    }

    /// Whether any of a box, given by its corners camera-relative, may be
    /// visible. Conservative: it can say yes for a box just outside a
    /// corner of the view, but never no for one inside.
    #[must_use]
    pub fn intersects_box(&self, min: Vec3, max: Vec3) -> bool {
        self.planes.iter().all(|plane| {
            // The box corner furthest along the plane's normal.
            let corner = Vec3::new(
                if plane.x >= 0.0 { max.x } else { min.x },
                if plane.y >= 0.0 { max.y } else { min.y },
                if plane.z >= 0.0 { max.z } else { min.z },
            );
            plane.truncate().dot(corner) + plane.w >= 0.0
        })
    }
}

#[cfg(test)]
mod tests {
    use pumpkin_protocol::{bedrock::server::player_auth_input::SPlayerAuthInput, serial::PacketRead};

    use super::*;

    /// The official client's own `camera_orientation` for its pitch and yaw,
    /// from the M2 golden fixtures.
    #[test]
    fn look_direction_matches_the_official_client() {
        let fixtures: [&[u8]; 3] = [
            include_bytes!(
                "../../../pumpkin/crates/pumpkin-protocol/src/bedrock/client_codecs/fixtures/player_auth_input-idle.bin"
            ),
            include_bytes!(
                "../../../pumpkin/crates/pumpkin-protocol/src/bedrock/client_codecs/fixtures/player_auth_input-walking.bin"
            ),
            include_bytes!(
                "../../../pumpkin/crates/pumpkin-protocol/src/bedrock/client_codecs/fixtures/player_auth_input-jumping.bin"
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

    #[test]
    fn angles_follow_bedrock() {
        let close = |a: Vec3, b: Vec3| (a - b).length() < 1e-5;
        assert!(close(look_direction(0.0, 0.0), Vec3::Z), "yaw 0 faces +z");
        assert!(close(look_direction(0.0, -90.0), Vec3::X), "yaw -90 faces +x");
        assert!(close(look_direction(90.0, 0.0), -Vec3::Y), "pitch 90 looks down");
    }

    #[test]
    fn flying_follows_the_yaw_and_turning_is_clamped() {
        let mut camera = Camera::new(DVec3::ZERO, 45.0, -90.0);
        let forward = Movement {
            forward: 1.0,
            ..Movement::default()
        };
        camera.fly(forward, 1.0);
        assert!((camera.position - DVec3::new(FLY_SPEED, 0.0, 0.0)).length() < 1e-9, "east, level");
        // Facing east, right is south (+z).
        let right = Movement {
            right: 1.0,
            fast: true,
            ..Movement::default()
        };
        let mut camera = Camera::new(DVec3::ZERO, 0.0, -90.0);
        camera.fly(right, 0.5);
        assert!((camera.position - DVec3::new(0.0, 0.0, FLY_SPEED * FAST * 0.5)).length() < 1e-9);

        camera.turn(0.0, 10_000.0);
        assert!((camera.pitch - MAX_PITCH).abs() < 1e-4);
        camera.turn(2400.0, 0.0); // 360 degrees
        assert!((camera.yaw + 90.0).abs() < 1e-3);
    }

    #[test]
    fn the_projection_maps_near_to_0_and_far_to_1() {
        let camera = Camera::new(DVec3::ZERO, 0.0, 0.0);
        let clip = |distance: f32| {
            let p = camera.view_projection() * Vec4::new(0.0, 0.0, distance, 1.0);
            p.z / p.w
        };
        assert!(clip(NEAR).abs() < 1e-4);
        assert!((clip(FAR) - 1.0).abs() < 1e-4);
    }

    #[test]
    fn the_frustum_keeps_what_is_in_front() {
        // Facing +z.
        let frustum = Camera::new(DVec3::ZERO, 0.0, 0.0).frustum();
        let cube = |at: Vec3| frustum.intersects_box(at, at + Vec3::splat(16.0));
        assert!(cube(Vec3::new(-8.0, -8.0, 20.0)), "straight ahead");
        assert!(!cube(Vec3::new(-8.0, -8.0, -40.0)), "behind");
        assert!(!cube(Vec3::new(-8.0, 500.0, 40.0)), "far above the view");
        assert!(!cube(Vec3::new(-8.0, -8.0, 4000.0)), "past the far plane");
        assert!(cube(Vec3::new(-8.0, -8.0, -8.0)), "around the camera");
    }
}
