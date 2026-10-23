//! CPU meshing of one sub-chunk (m4-plan Decision 6): the visible faces of
//! its blocks, as quads ready for the GPU.
//!
//! **A padded neighbourhood** (worth teaching). Whether a face is drawn
//! depends on the block beside it, and for blocks on the sub-chunk's edge
//! that block is in another sub-chunk. So [`Neighbourhood`] holds the 16³
//! blocks plus a one-block border copied from the six neighbours: 18³ cells.
//! The mesher then reads `x ± 1` for any block without a bounds check or a
//! lookup in another chunk. The border's edges and corners stay air: only
//! the six face neighbours decide anything.
//!
//! **Translate the palette, not each block** (worth teaching). A sub-chunk
//! layer stores a short palette of block ids and a small index per
//! position. Looking up each palette entry's appearance once, then copying
//! indices, does 4,096 cheap copies instead of 4,096 hash lookups.
//!
//! The mesh keeps culled faces and skips greedy merging (M6 does better on
//! the GPU). Its texture orientation is what `blocks::appearance` assumes:
//! side faces put `v` = 0 at the top, the up face runs `u` along +x and `v`
//! along +z, and the down face runs `v` along −z.

use std::collections::HashMap;

use limonite_packs::registry::Face;

use crate::{
    blocks::appearance::{Appearance, AppearanceTable, FaceLook, Liquid, RenderClass, Shape},
    render::tint::{Tint, Tints},
    world::{AIR, World},
};

/// The neighbourhood's width: 16 blocks and a border block on each side.
pub const PAD: usize = 18;
const CELLS: usize = PAD * PAD * PAD;

/// A cell's index from padded coordinates, each 0 to 17. x is innermost, so
/// a row of blocks is contiguous.
const fn cell(x: usize, y: usize, z: usize) -> usize {
    (y * PAD + z) * PAD + x
}

/// The step to the neighbouring cell in each face's direction.
const STEP: [isize; 6] = [
    -((PAD * PAD) as isize), // down: y - 1
    (PAD * PAD) as isize,    // up: y + 1
    -(PAD as isize),         // north: z - 1
    PAD as isize,            // south: z + 1
    -1,                      // west: x - 1
    1,                       // east: x + 1
];

/// The face code of the diagonal planes of a cross. Faces are 0 to 5, in
/// [`Face`] order.
pub const DIAGONAL: u8 = 6;

/// One corner of a quad: 16 bytes (m4-plan Decision 6).
///
/// `#[repr(C)]` fixes the field order and padding, so the GPU reads it with
/// the layout `terrain.rs` declares. `Pod` (plain old data) promises
/// `bytemuck` that any bytes are a valid `Vertex` and the struct has no
/// padding, so a `Vec<Vertex>` can be handed to the GPU as bytes, uncopied.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    /// The position in the sub-chunk, in sixteenths of a block: 0 to 256.
    pub position: [u16; 3],
    /// The texture's layer in the array.
    pub layer: u16,
    /// Texture coordinates in sixteenths of the texture: 0 to 16.
    pub uv: [u8; 2],
    /// The face (bits 0 to 2: [`Face`] order, or [`DIAGONAL`]) and the render
    /// class (bits 3 and 4: opaque, cutout, overlay, blend).
    pub face_class: u8,
    pub unused: u8,
    /// The tint, RGBA. White leaves the texture as it is.
    pub tint: [u8; 4],
}

impl Vertex {
    #[cfg(test)]
    #[must_use]
    pub const fn face(&self) -> u8 {
        self.face_class & 0b111
    }

    #[cfg(test)]
    #[must_use]
    pub const fn class(&self) -> u8 {
        self.face_class >> 3
    }
}

/// One sub-chunk's quads, four vertices each, in two lists for the two
/// passes: opaque, cutout and overlay faces write depth; blended faces are
/// drawn after, back to front.
#[derive(Debug, Default)]
pub struct Mesh {
    pub opaque: Vec<Vertex>,
    pub blend: Vec<Vertex>,
}

impl Mesh {
    #[must_use]
    pub fn quads(&self) -> usize {
        (self.opaque.len() + self.blend.len()) / 4
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.opaque.is_empty() && self.blend.is_empty()
    }
}

/// A sub-chunk and a one-block border from its neighbours, as appearances.
pub struct Neighbourhood {
    /// The sub-chunk's lowest corner, in world blocks.
    pub origin: [i32; 3],
    /// Each distinct appearance here. Index 0 is air.
    looks: Vec<Appearance>,
    /// Which id each look is, for interning.
    ids: HashMap<u32, u16>,
    /// For each of the two layers, every cell's index into `looks`.
    cells: [Box<[u16]>; 2],
    /// The biome of each column, at `x + 16 z`.
    biomes: [u32; 256],
}

impl Neighbourhood {
    /// An all-air neighbourhood.
    fn empty(origin: [i32; 3], table: &AppearanceTable) -> Self {
        Self {
            origin,
            looks: vec![*table.get(AIR)],
            ids: HashMap::from([(AIR, 0)]),
            cells: [vec![0; CELLS].into_boxed_slice(), vec![0; CELLS].into_boxed_slice()],
            // Plains, until a chunk says otherwise.
            biomes: [1; 256],
        }
    }

    /// The index of an id's look, adding it the first time.
    fn intern(&mut self, table: &AppearanceTable, id: u32) -> u16 {
        if let Some(&index) = self.ids.get(&id) {
            return index;
        }
        let index = self.looks.len() as u16;
        self.looks.push(*table.get(id));
        self.ids.insert(id, index);
        index
    }

    fn set(&mut self, table: &AppearanceTable, layer: usize, at: usize, id: u32) {
        self.cells[layer][at] = self.intern(table, id);
    }

    /// Sub-chunk `section` (counted from the chunk's lowest) of chunk
    /// (`chunk_x`, `chunk_z`), with its border. `None` if the chunk or one of
    /// its four side neighbours isn't loaded: a border would be missing, and
    /// faces against it would be guesses.
    #[must_use]
    pub fn gather(
        world: &World,
        table: &AppearanceTable,
        chunk_x: i32,
        section: usize,
        chunk_z: i32,
    ) -> Option<Self> {
        let chunk = world.chunk(chunk_x, chunk_z)?;
        let north = world.chunk(chunk_x, chunk_z - 1)?;
        let south = world.chunk(chunk_x, chunk_z + 1)?;
        let west = world.chunk(chunk_x - 1, chunk_z)?;
        let east = world.chunk(chunk_x + 1, chunk_z)?;
        let sub_chunk = chunk.sub_chunks.get(section)?;
        let base_y = (chunk.min_section + section as i32) * 16;
        let mut n = Self::empty([chunk_x * 16, base_y, chunk_z * 16], table);

        for (layer, storage) in sub_chunk.layers.iter().enumerate().take(2) {
            let palette: Vec<u16> = storage
                .palette()
                .iter()
                .map(|&id| n.intern(table, id))
                .collect();
            let cells = &mut n.cells[layer];
            for y in 0..16 {
                for z in 0..16 {
                    for x in 0..16 {
                        cells[cell(x + 1, y + 1, z + 1)] = palette[storage.palette_index(x, y, z)];
                    }
                }
            }
        }

        for layer in 0..2 {
            for a in 0..16 {
                for b in 0..16 {
                    let y = base_y + b as i32;
                    // Below and above, from the same chunk (a = x, b = z).
                    n.set(table, layer, cell(a + 1, 0, b + 1), chunk.block(a, base_y - 1, b, layer));
                    n.set(table, layer, cell(a + 1, 17, b + 1), chunk.block(a, base_y + 16, b, layer));
                    // North and south (a = x, b = y).
                    n.set(table, layer, cell(a + 1, b + 1, 0), north.block(a, y, 15, layer));
                    n.set(table, layer, cell(a + 1, b + 1, 17), south.block(a, y, 0, layer));
                    // West and east (a = z, b = y).
                    n.set(table, layer, cell(0, b + 1, a + 1), west.block(15, y, a, layer));
                    n.set(table, layer, cell(17, b + 1, a + 1), east.block(0, y, a, layer));
                }
            }
        }

        // One biome per column: the sub-chunk's middle.
        for z in 0..16 {
            for x in 0..16 {
                if let Some(biome) = chunk.biome(x, base_y + 8, z) {
                    n.biomes[x + 16 * z] = biome;
                }
            }
        }
        Some(n)
    }

    /// A neighbourhood from a function of block coordinates relative to the
    /// sub-chunk (-1 to 16 on each axis; the border's edges are never asked
    /// for) and the layer.
    #[cfg(test)]
    pub fn from_fn(
        table: &AppearanceTable,
        origin: [i32; 3],
        biome: u32,
        id_at: impl Fn(i32, i32, i32, usize) -> u32,
    ) -> Self {
        let mut n = Self::empty(origin, table);
        n.biomes = [biome; 256];
        for layer in 0..2 {
            for y in 0..PAD {
                for z in 0..PAD {
                    for x in 0..PAD {
                        let edges = [x, y, z].iter().filter(|&&c| c == 0 || c == PAD - 1).count();
                        if edges <= 1 {
                            let id = id_at(x as i32 - 1, y as i32 - 1, z as i32 - 1, layer);
                            n.set(table, layer, cell(x, y, z), id);
                        }
                    }
                }
            }
        }
        n
    }
}

/// Meshes a neighbourhood's sub-chunk.
#[must_use]
pub fn mesh(n: &Neighbourhood, tints: &Tints) -> Mesh {
    let mut mesh = Mesh::default();
    for y in 1..=16 {
        for z in 1..=16 {
            for x in 1..=16 {
                let at = cell(x, y, z);
                for layer in 0..2 {
                    let index = n.cells[layer][at];
                    if index == 0 {
                        continue;
                    }
                    let look = &n.looks[usize::from(index)];
                    let block = Block {
                        n,
                        at,
                        local: [x as u16 - 1, y as u16 - 1, z as u16 - 1],
                        look,
                        biome: n.biomes[(x - 1) + 16 * (z - 1)],
                    };
                    block.emit(tints, &mut mesh);
                }
            }
        }
    }
    mesh
}

/// One block being meshed.
struct Block<'a> {
    n: &'a Neighbourhood,
    /// Its cell.
    at: usize,
    /// Its position in the sub-chunk, in blocks.
    local: [u16; 3],
    look: &'a Appearance,
    biome: u32,
}

impl Block<'_> {
    fn emit(&self, tints: &Tints, mesh: &mut Mesh) {
        match self.look.shape {
            Shape::Invisible => {}
            Shape::Cube => {
                for face in Face::ALL {
                    if !self.neighbour_occludes(face) {
                        self.quad(face, [0, 0, 0], [16, 16, 16], tints, mesh);
                    }
                }
            }
            Shape::Slab { bottom, top } => {
                for face in Face::ALL {
                    // Only faces on the block's boundary can be hidden.
                    let on_boundary = match face {
                        Face::Down => bottom == 0,
                        Face::Up => top == 16,
                        _ => true,
                    };
                    if !(on_boundary && self.neighbour_occludes(face)) {
                        self.quad(face, [0, bottom, 0], [16, top, 16], tints, mesh);
                    }
                }
            }
            Shape::Liquid { liquid, surface } => {
                let covered = self.neighbour_is(Face::Up, liquid);
                let top = if covered { 16 } else { surface };
                for face in Face::ALL {
                    let hidden = self.neighbour_is(face, liquid)
                        || (self.neighbour_occludes(face) && (face != Face::Up || top == 16));
                    if !hidden {
                        self.quad(face, [0, 0, 0], [16, top, 16], tints, mesh);
                    }
                }
            }
            Shape::Cross => self.cross(tints, mesh),
            Shape::Planes { faces } => {
                for face in Face::ALL.into_iter().filter(|&face| faces & (1 << face as u8) != 0) {
                    self.plane(face, tints, mesh);
                }
            }
        }
    }

    /// The look of the block beside this one, on `layer`.
    fn neighbour(&self, face: Face, layer: usize) -> &Appearance {
        let at = self.at.wrapping_add_signed(STEP[face as usize]);
        &self.n.looks[usize::from(self.n.cells[layer][at])]
    }

    fn neighbour_occludes(&self, face: Face) -> bool {
        self.neighbour(face, 0).occludes
    }

    /// Whether the block beside holds `liquid` on either layer: water around
    /// a waterlogged block counts.
    fn neighbour_is(&self, face: Face, liquid: Liquid) -> bool {
        self.neighbour(face, 0).liquid() == Some(liquid) || self.neighbour(face, 1).liquid() == Some(liquid)
    }

    /// The colour a face of this block is multiplied by.
    fn tint(&self, look: &FaceLook, tints: &Tints) -> [u8; 4] {
        if look.tinted && self.look.tint != Tint::None {
            tints.color(self.biome, self.look.tint)
        } else {
            [255; 4]
        }
    }

    /// One face of a box from `min` to `max`, in sixteenths of the block.
    fn quad(&self, face: Face, min: [u8; 3], max: [u8; 3], tints: &Tints, mesh: &mut Mesh) {
        let look = self.look.face(face);
        let corners = box_face(face, min, max);
        let mut uvs = corners.map(|corner| face_uv(face, corner));
        let mut turns = look.turns;
        if look.isotropic {
            let [x, y, z] = self.world_position();
            turns += isotropic_turns(x, y, z, face as u8);
        }
        for _ in 0..turns % 4 {
            uvs = uvs.map(quarter_turn);
        }
        self.push(corners, uvs, face as u8, look, tints, mesh);
    }

    /// Two planes crossed on the diagonals, each drawn from both sides. They
    /// run from 1/16 to 15/16, as Java's rescaled `cross` model does on a
    /// sixteenth grid.
    fn cross(&self, tints: &Tints, mesh: &mut Mesh) {
        let look = self.look.face(Face::North);
        let uvs = [[0, 0], [0, 16], [16, 16], [16, 0]];
        for (start, end) in [([1, 1], [15, 15]), ([1, 15], [15, 1])] {
            let front = [
                [start[0], 16, start[1]],
                [start[0], 0, start[1]],
                [end[0], 0, end[1]],
                [end[0], 16, end[1]],
            ];
            // The back: the same corners and texture, in the other order.
            let back = [front[3], front[2], front[1], front[0]];
            let back_uvs = [uvs[3], uvs[2], uvs[1], uvs[0]];
            self.push(front, uvs, DIAGONAL, look, tints, mesh);
            self.push(back, back_uvs, DIAGONAL, look, tints, mesh);
        }
    }

    /// A plane a sixteenth inside `face`, drawn from both sides: a vine or
    /// lichen on the block beside it.
    fn plane(&self, face: Face, tints: &Tints, mesh: &mut Mesh) {
        let look = self.look.face(face);
        // A box of no thickness, one sixteenth in from the face.
        let (axis, depth) = match face {
            Face::Down => (1, 1),
            Face::Up => (1, 15),
            Face::North => (2, 1),
            Face::South => (2, 15),
            Face::West => (0, 1),
            Face::East => (0, 15),
        };
        let (mut min, mut max) = ([0u8; 3], [16u8; 3]);
        min[axis] = depth;
        max[axis] = depth;
        let corners = box_face(face, min, max);
        let uvs = corners.map(|corner| face_uv(face, corner));
        let back = [corners[3], corners[2], corners[1], corners[0]];
        let back_uvs = [uvs[3], uvs[2], uvs[1], uvs[0]];
        self.push(corners, uvs, face as u8, look, tints, mesh);
        self.push(back, back_uvs, face as u8, look, tints, mesh);
    }

    /// Adds one quad: block-relative corners in sixteenths.
    fn push(
        &self,
        corners: [[u8; 3]; 4],
        uvs: [[u8; 2]; 4],
        face: u8,
        look: &FaceLook,
        tints: &Tints,
        mesh: &mut Mesh,
    ) {
        let class = match look.class {
            RenderClass::Opaque => 0,
            RenderClass::Cutout => 1,
            RenderClass::Overlay => 2,
            RenderClass::Blend => 3,
        };
        let tint = self.tint(look, tints);
        let out = if look.class == RenderClass::Blend { &mut mesh.blend } else { &mut mesh.opaque };
        for (corner, uv) in corners.into_iter().zip(uvs) {
            out.push(Vertex {
                position: [0, 1, 2].map(|axis| self.local[axis] * 16 + u16::from(corner[axis])),
                layer: look.layer,
                uv,
                face_class: face | class << 3,
                unused: 0,
                tint,
            });
        }
    }

    fn world_position(&self) -> [i32; 3] {
        [0, 1, 2].map(|axis| self.n.origin[axis] + i32::from(self.local[axis]))
    }
}

/// A box face's four corners, counter-clockwise seen from outside, starting
/// at the texture's top left. Counter-clockwise is the front face the GPU
/// keeps; it culls the back of every quad.
const fn box_face(face: Face, min: [u8; 3], max: [u8; 3]) -> [[u8; 3]; 4] {
    let ([x0, y0, z0], [x1, y1, z1]) = (min, max);
    match face {
        Face::Up => [[x0, y1, z0], [x0, y1, z1], [x1, y1, z1], [x1, y1, z0]],
        Face::Down => [[x0, y0, z1], [x0, y0, z0], [x1, y0, z0], [x1, y0, z1]],
        Face::North => [[x1, y1, z0], [x1, y0, z0], [x0, y0, z0], [x0, y1, z0]],
        Face::South => [[x0, y1, z1], [x0, y0, z1], [x1, y0, z1], [x1, y1, z1]],
        Face::West => [[x0, y1, z0], [x0, y0, z0], [x0, y0, z1], [x0, y1, z1]],
        Face::East => [[x1, y1, z1], [x1, y0, z1], [x1, y0, z0], [x1, y1, z0]],
    }
}

/// A corner's texture coordinates on `face`, from its position: a slab's
/// side shows the matching part of the texture.
const fn face_uv(face: Face, [x, y, z]: [u8; 3]) -> [u8; 2] {
    match face {
        Face::Up => [x, z],
        Face::Down => [x, 16 - z],
        Face::North => [16 - x, 16 - y],
        Face::South => [x, 16 - y],
        Face::West => [z, 16 - y],
        Face::East => [16 - z, 16 - y],
    }
}

/// Turns texture coordinates a quarter around the texture's centre.
const fn quarter_turn([u, v]: [u8; 2]) -> [u8; 2] {
    [16 - v, u]
}

/// A quarter-turn count picked by position, the same on every run. The
/// position mixing is Java's `Mth.getSeed`; Bedrock's own pattern isn't
/// known, so it won't match the official client tile for tile.
fn isotropic_turns(x: i32, y: i32, z: i32, face: u8) -> u8 {
    let mut seed = i64::from(x.wrapping_mul(3_129_871)) ^ i64::from(z).wrapping_mul(116_129_781) ^ i64::from(y);
    seed = seed
        .wrapping_mul(seed)
        .wrapping_mul(42_317_861)
        .wrapping_add(seed.wrapping_mul(11));
    ((seed >> 16) as u8).wrapping_add(face) & 3
}

#[cfg(test)]
mod tests;
