//! The renderer: the GPU device and the window's surface, the camera, and
//! the CPU work that feeds them (textures, tints, meshes).
//!
//! **Capabilities, never vendors** (CLAUDE.md; m4-plan Decision 11). Every
//! decision here reads what the adapter and surface report: limits, formats
//! and present modes. The adapter's name, vendor and device type are logged
//! for context, and no code path reads them.

pub mod camera;
pub mod mesher;
pub mod textures;
pub mod tint;

use std::sync::Arc;

use limonite_packs::PackStack;
use tracing::{info, warn};
use winit::{event_loop::OwnedDisplayHandle, window::Window};

use self::tint::Tints;

/// What the renderer asks of the GPU.
pub struct Needs {
    /// Texture array layers the block textures take.
    pub texture_layers: u32,
    /// Prefer a present mode without vsync, for frame-time measurements.
    pub uncapped: bool,
}

/// The device, its queue, and the window's surface.
pub struct Gpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    pub config: wgpu::SurfaceConfiguration,
}

/// What became of one frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame {
    Drawn,
    /// Nothing to draw into right now: minimised, hidden, or a timeout.
    Skipped,
}

impl Gpu {
    /// Picks an adapter and creates a device that can present to `window`.
    ///
    /// # Errors
    /// No adapter can present to the window, or one can't meet [`Needs`].
    /// The message names what's missing and both numbers.
    pub fn new(window: Arc<Window>, display: OwnedDisplayHandle, needs: &Needs) -> Result<Self, String> {
        // The display handle ties the instance to the window system, which
        // GLES and Wayland need. `from_env` reads `WGPU_BACKEND`, which
        // forces a backend (`vulkan`, `dx12`, ...) for testing.
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_with_display_handle_from_env(
            Box::new(display),
        ));
        let surface = instance
            .create_surface(window.clone())
            .map_err(|err| format!("can't create a surface for the window: {err}"))?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        }))
        .map_err(|err| format!("no GPU adapter can draw to this window: {err}"))?;

        let adapter_info = adapter.get_info();
        info!(
            "GPU adapter: {} ({:?}, {:?}); driver {} {}",
            adapter_info.name,
            adapter_info.backend,
            adapter_info.device_type,
            adapter_info.driver,
            adapter_info.driver_info,
        );

        // The one limit above WebGPU's defaults: a layer per block texture.
        // WebGPU guarantees 256 layers, and desktop adapters report 2,048 or
        // more. So ask for the adapter's own limit, and fail with numbers if
        // it's too small; smaller adapters wait for M6's tiers.
        let layers = adapter.limits().max_texture_array_layers;
        if layers < needs.texture_layers {
            return Err(format!(
                "this GPU's max_texture_array_layers is {layers}, but the block textures need {}",
                needs.texture_layers
            ));
        }
        let required_limits = wgpu::Limits {
            max_texture_array_layers: layers,
            ..wgpu::Limits::default()
        };
        // Everything else M4 uses is core WebGPU: no optional features.
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("limonite"),
            required_features: wgpu::Features::empty(),
            required_limits,
            ..Default::default()
        }))
        .map_err(|err| format!("the GPU adapter wouldn't create a device: {err}"))?;

        let capabilities = surface.get_capabilities(&adapter);
        let format = capabilities
            .formats
            .iter()
            .copied()
            .find(wgpu::TextureFormat::is_srgb)
            .or_else(|| {
                let first = capabilities.formats.first().copied();
                warn!("The surface offers no sRGB format, so colours will be off; using {first:?}");
                first
            })
            .ok_or("the surface offers no formats")?;
        let present_mode = choose_present_mode(&capabilities.present_modes, needs.uncapped);
        let size = window.inner_size();
        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .ok_or("the adapter can't present to this surface")?;
        config.format = format;
        config.present_mode = present_mode;
        surface.configure(&device, &config);
        info!(
            "Surface: {format:?}, {present_mode:?} (offered {:?}); {} texture array layers allowed",
            capabilities.present_modes, layers
        );
        Ok(Self {
            device,
            queue,
            surface,
            config,
        })
    }

    /// Follows the window's new size. A zero size (minimised) is skipped:
    /// a surface can't be zero pixels, and nothing is drawn meanwhile.
    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
    }

    /// Acquires the next surface texture, lets `draw` record into it, then
    /// submits and presents. A surface that needs reconfiguring is
    /// reconfigured, and the frame skipped.
    pub fn frame(&mut self, draw: impl FnOnce(&Self, &wgpu::TextureView, &mut wgpu::CommandEncoder)) -> Frame {
        let (texture, suboptimal) = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture) => (texture, false),
            wgpu::CurrentSurfaceTexture::Suboptimal(texture) => (texture, true),
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Frame::Skipped;
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                return Frame::Skipped;
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                warn!("Couldn't get the next frame: a validation error (logged by wgpu)");
                return Frame::Skipped;
            }
        };
        let view = texture.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });
        draw(self, &view, &mut encoder);
        self.queue.submit([encoder.finish()]);
        self.queue.present(texture);
        if suboptimal {
            self.surface.configure(&self.device, &self.config);
        }
        Frame::Drawn
    }
}

/// Fifo, vsync, is always supported, and the default. Measuring frame times
/// wants the GPU uncapped: Immediate if the surface offers it, else Mailbox,
/// else Fifo after all.
fn choose_present_mode(offered: &[wgpu::PresentMode], uncapped: bool) -> wgpu::PresentMode {
    let wanted: &[wgpu::PresentMode] = if uncapped {
        &[wgpu::PresentMode::Immediate, wgpu::PresentMode::Mailbox]
    } else {
        &[]
    };
    wanted
        .iter()
        .copied()
        .find(|mode| offered.contains(mode))
        .unwrap_or(wgpu::PresentMode::Fifo)
}

/// The sky's colour, the clear colour for now: Java's plains sky, #78A7FF,
/// in linear light for an sRGB surface.
pub const SKY: wgpu::Color = wgpu::Color {
    r: 0.188,
    g: 0.386,
    b: 1.0,
    a: 1.0,
};

/// Loads the biome tints, logging the colormaps, the climate table and the
/// water colour.
pub fn load_tints(stack: &PackStack) -> Tints {
    let (tints, report) = Tints::load(stack);
    let maps: Vec<String> = report
        .colormaps
        .iter()
        .map(|(path, size)| {
            let name = path.rsplit('/').next().unwrap_or(path);
            match size {
                Ok((width, height)) => format!("{name} {width}×{height}"),
                Err(err) => {
                    warn!("Colormap {path} didn't load, so its tint is white: {err}");
                    format!("{name} missing")
                }
            }
        })
        .collect();
    let water = match &report.water {
        Ok([r, g, b, a]) => format!("#{r:02X}{g:02X}{b:02X} at {:.0}% opacity", f32::from(*a) / 2.55),
        Err(err) => {
            warn!("No water colour, so water is white: {err}");
            "missing".to_string()
        }
    };
    info!(
        "Tints: {} Bedrock biome ids with a climate ({} Java biomes share an id with a lower one); \
         colormaps {}; water {water}",
        report.biomes,
        report.collisions,
        maps.join(", "),
    );
    tints
}

#[cfg(test)]
mod tests {
    use wgpu::PresentMode;

    use super::*;

    #[test]
    fn present_modes_fall_back_to_fifo() {
        let all = [PresentMode::Fifo, PresentMode::Mailbox, PresentMode::Immediate];
        assert_eq!(choose_present_mode(&all, false), PresentMode::Fifo);
        assert_eq!(choose_present_mode(&all, true), PresentMode::Immediate);
        assert_eq!(choose_present_mode(&[PresentMode::Fifo, PresentMode::Mailbox], true), PresentMode::Mailbox);
        assert_eq!(choose_present_mode(&[PresentMode::Fifo], true), PresentMode::Fifo);
    }
}
