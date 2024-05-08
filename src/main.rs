/*

TODO:
 - Finish draw_corners shader
 - Test linking OrbProgram buffer

*/

use std::sync::Arc;

use bytemuck::Zeroable;
use pollster::FutureExt;
use wgpu::BufferUsages;
use winit::{
    dpi::PhysicalSize, event::{Event, WindowEvent}, event_loop::EventLoop, window::Window
};

use nokhwa::{pixel_format::RgbAFormat, utils::RequestedFormat, Camera};

use tinyslam::orb::{CornerData, CornerDescriptor, OrbConfig, OrbProgram};

use tiny_wgpu::{
    BindGroupItem, Compute, ComputeProgram, RenderKernel, Storage
};

struct VisualizationProgram<'a> {
    pub surface: wgpu::Surface<'a>,

    pub image_size: wgpu::Extent3d,

    storage: Storage,
    compute: &'a Compute,

    orb_storage: &'a Storage
}

impl<'a> ComputeProgram for VisualizationProgram<'a> {
    fn compute(&self) -> &Compute {
        self.compute
    }
    
    fn storage(&self) -> &Storage {
        &self.storage
    }

    fn storage_mut(&mut self) -> &mut Storage {
        &mut self.storage
    }
}

impl<'a> VisualizationProgram<'a> {    
    pub fn init(&mut self) {
        self.add_module("blit", wgpu::include_wgsl!("shaders/blit.wgsl"));
        self.add_module("draw_corners", wgpu::include_wgsl!("shaders/draw_corners.wgsl"));

        self.add_texture(
            "visualization",
            wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_DST,
            wgpu::TextureFormat::Rgba8Unorm,
            self.image_size,
        );

        self.add_sampler(
            "linear_sampler",
            wgpu::SamplerDescriptor {
                label: None,
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                address_mode_w: wgpu::AddressMode::ClampToEdge,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::FilterMode::Linear,
                lod_max_clamp: 1.0,
                lod_min_clamp: 0.0,
                compare: None,
                anisotropy_clamp: 1,
                border_color: None
            }
        );

        self.add_buffer(
            "base_resolution", 
            BufferUsages::UNIFORM | BufferUsages::COPY_DST, 
            4 * 2
        );

        {
            self.compute().queue.write_buffer(
                &self.storage().buffers["base_resolution"], 
                0,
                bytemuck::cast_slice(&[ self.image_size.width, self.image_size.height ])
            );
        }

        self.add_bind_group("blit_to_screen", &[
            BindGroupItem::Sampler { label: "linear_sampler" },
            BindGroupItem::Texture { label: "visualization" }
        ]);

        let swapchain_capabilities = self.surface.get_capabilities(&self.compute().adapter);
        let swapchain_format = swapchain_capabilities.formats[0];

        self.add_render_pipelines(
            "blit",
            &["blit_to_screen"],
            &[RenderKernel { label: "blit_to_screen", vertex: "vs_main", fragment: "fs_main" }],
            &[],
            &[Some(swapchain_format.into())],
            &[],
            None,
            None
        );

        self.add_bind_group("base_resolution", &[
            BindGroupItem::UniformBuffer { label: "base_resolution", min_binding_size: 8 }
        ]);

        self.add_render_pipelines(
            "draw_corners",
            &["base_resolution"], 
            &[RenderKernel { label: "draw_corners", vertex: "vs_main", fragment: "fs_main" }], 
            &[], 
            &[Some(self.storage().textures["visualization"].format().into())], 
            &[wgpu::VertexBufferLayout {
                array_stride: 4 * 4,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &[
                    // X
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Uint32,
                        offset: 0,
                        shader_location: 0
                    },
                    // Y
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Uint32,
                        offset: 4,
                        shader_location: 1
                    },
                    // Angle
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Uint32,
                        offset: 8,
                        shader_location: 2
                    },
                    // Octave
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Uint32,
                        offset: 12,
                        shader_location: 3
                    },
                ]
            }], 
            None, 
            None
        );
    }

    pub fn run(&self, num_corners: u32) {

        let mut encoder = self.compute().device.create_command_encoder(&Default::default());

        encoder.copy_texture_to_texture(
            wgpu::ImageCopyTextureBase { 
                texture: &self.orb_storage.textures["input_image"], 
                mip_level: 0, 
                origin: wgpu::Origin3d::ZERO, 
                aspect: wgpu::TextureAspect::All
            }, 
            wgpu::ImageCopyTextureBase { 
                texture: &self.storage().textures["visualization"], 
                mip_level: 0, 
                origin: wgpu::Origin3d::ZERO, 
                aspect: wgpu::TextureAspect::All
            }, 
            self.image_size
        );

        if num_corners > 0 {
            {
                let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor { 
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment { 
                        view: &self.storage().texture_views["visualization"], 
                        resolve_target: None, 
                        ops: wgpu::Operations { 
                            load: wgpu::LoadOp::Load, 
                            store: wgpu::StoreOp::Store
                        } 
                    })], 
                    ..Default::default()
                });

                rpass.set_pipeline(&self.storage().render_pipelines["draw_corners"]);
                rpass.set_bind_group(0, &self.storage().bind_groups["base_resolution"], &[]);
                rpass.set_vertex_buffer(0, self.orb_storage.buffers["corners"].slice(..(num_corners as u64 * 4 * 4)));
                rpass.draw(0..6, 0..num_corners);
            }
        }

        let frame = self.surface.get_current_texture().unwrap();
        let view = frame.texture.create_view(&Default::default());

        {
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(wgpu::RenderPassColorAttachment { 
                    view: &view,
                    resolve_target: None, 
                    ops: wgpu::Operations { 
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), 
                        store: wgpu::StoreOp::Store
                    } 
                })],
                ..Default::default()
            });

            rpass.set_pipeline(&self.storage().render_pipelines["blit_to_screen"]);
            rpass.set_bind_group(0, &self.storage().bind_groups["blit_to_screen"], &[]);
            rpass.draw(0..3, 0..1);
        }

        self.compute().queue.submit(Some(encoder.finish()));

        frame.present();
    }

    fn configure_surface(&self, width: u32, height: u32) {
        let config = self
            .surface
            .get_default_config(&self.compute().adapter, width, height)
            .unwrap();
    
        self.surface.configure(&self.compute().device, &config);
    }
}

fn run(
    event_loop: EventLoop<()>,
    window: Arc<Window>,
) -> Result<(), winit::error::EventLoopError> {
    let mut camera = {
        let index = nokhwa::utils::CameraIndex::Index(0);
        let requested_format = nokhwa::utils::RequestedFormatType::AbsoluteHighestResolution;
        type Decoder = nokhwa::pixel_format::RgbAFormat;
        let format = RequestedFormat::new::<Decoder>(requested_format);

        let mut camera = Camera::new(index, format).unwrap();

        camera.open_stream().expect("Could not open stream.");

        camera
    };

    let (frame_width, frame_height) = {
        let frame = camera.frame().unwrap();
        let resolution = frame.resolution();

        (resolution.width(), resolution.height())
    };

    let mut frame_buffer = vec![0u8; (frame_width * frame_height * 4) as usize];

    let _ = window.request_inner_size(PhysicalSize {
        width: frame_width,
        height: frame_height
    });

    let orb_program = {
        let mut orb_program = OrbProgram {
            config: OrbConfig {
                max_features: 4096,
                image_size: wgpu::Extent3d { 
                    width: frame_width, 
                    height: frame_height, 
                    depth_or_array_layers: 1
                },
                hierarchy_depth: 3,
                initial_threshold: 0.4,
            },
            compute: Compute::new(
                wgpu::Features::PUSH_CONSTANTS,
                {
                    let mut limits = wgpu::Limits::default();
                    limits.max_push_constant_size = 4;
                    limits.max_storage_buffers_per_shader_stage = 8;
                    limits.max_texture_dimension_1d = 4096;
                    limits.max_texture_dimension_2d = 4096;
                    limits
                }
                
            ).block_on(),
            storage: Default::default()
        };

        orb_program.init();
        orb_program
    };

    let visualization_program = {
        let mut visualization_program = VisualizationProgram {
            compute: orb_program.compute(),
            surface: orb_program.compute().instance.create_surface(&window).unwrap(),
            storage: Default::default(),
            orb_storage: orb_program.storage(),
            image_size: wgpu::Extent3d {
                width: frame_width,
                height: frame_height,
                depth_or_array_layers: 1
            }
        };
    
        visualization_program.init();

        visualization_program.configure_surface(frame_width, frame_height);

        visualization_program
    };

    let window = &window;
    let orb_program = &orb_program;

    event_loop.run(move |event, target| {

        let Event::WindowEvent { event, .. } = event else { return; };

        match event {
            WindowEvent::Resized(new_size) => {
                visualization_program.configure_surface(new_size.width, new_size.height);
                window.request_redraw();
            },
            WindowEvent::RedrawRequested => {
                let new_camera_frame = camera.frame().unwrap();

                new_camera_frame
                    .decode_image_to_buffer::<RgbAFormat>(&mut frame_buffer)
                    .unwrap();

                orb_program.write_input_image(&frame_buffer);

                let corner_count = orb_program.extract_corners();

                // Read corner data
                let mut corners = vec![CornerData::zeroed(); corner_count as usize];
                let mut descriptors = vec![CornerDescriptor::zeroed(); corner_count as usize];

                orb_program.read_corners(&mut corners);
                orb_program.read_descriptors(&mut descriptors);

                // TODO: Do something with corners / descriptors

                println!("Detected {} corners.", corner_count);

                visualization_program.run(corner_count);

                window.request_redraw();
            },
            WindowEvent::CloseRequested => {
                target.exit();
            },
            _ => {}
        }
        
    })
}

fn main() -> Result<(), winit::error::EventLoopError> {
    std::env::set_var("RUST_BACKTRACE", "1");

    let event_loop = EventLoop::new().unwrap();
    let window = Window::new(&event_loop).unwrap();
    window.set_title("tinyslam example");
    run(event_loop, Arc::new(window))
}
