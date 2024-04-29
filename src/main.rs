use std::sync::Arc;

use pollster::FutureExt;
use winit::{
    dpi::PhysicalSize, event::{Event, WindowEvent}, event_loop::EventLoop, window::Window
};

use nokhwa::{pixel_format::RgbAFormat, utils::RequestedFormat, Camera};

use tinyslam::orb::{OrbConfig, OrbParams, OrbProgram};

use tiny_wgpu::{
    BindGroupItem, Compute, ComputeProgram, RenderKernel, Storage
};

struct VisualizationProgram<'a> {
    pub surface: wgpu::Surface<'a>,

    storage: Storage,
    compute: &'a Compute
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
    pub fn init(
        &mut self,
        output_image_size: wgpu::Extent3d
    ) {
        self.add_module("draw_texture", wgpu::include_wgsl!("draw_texture.wgsl"));

        self.add_texture(
            "texture",
            wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            wgpu::TextureFormat::Rgba8Unorm,
            output_image_size,
        );

        self.add_bind_group(
            "draw_texture",
            &[BindGroupItem::Texture { label: "texture" }],
        );

        let swapchain_capabilities = self.surface.get_capabilities(&self.compute().adapter);
        let swapchain_format = swapchain_capabilities.formats[0];

        self.add_render_pipelines(
            "draw_texture",
            &["draw_texture"],
            &[RenderKernel { label: "draw_texture", vertex: "vs_main", fragment: "fs_main" }],
            &[],
            &[Some(swapchain_format.into())],
            &[],
            None,
            None
        );
    }
}

fn run(
    event_loop: EventLoop<()>,
    window: Arc<Window>,
) -> Result<(), winit::error::EventLoopError> {
    let mut camera = {
        let mut camera = Camera::new(
            nokhwa::utils::CameraIndex::Index(0),
            RequestedFormat::new::<nokhwa::pixel_format::RgbAFormat>(
                nokhwa::utils::RequestedFormatType::AbsoluteHighestResolution,
            ),
        )
        .unwrap();

        // println!(
        //     "Frame rate: {}",
        //     camera.refresh_camera_format().unwrap().frame_rate()
        // );

        // let mut formats = camera.compatible_camera_formats().unwrap();

        // formats.sort_by(|a, b| {
        //     if a.frame_rate() > b.frame_rate() {
        //         std::cmp::Ordering::Greater
        //     } else if a.frame_rate() < b.frame_rate() {
        //         std::cmp::Ordering::Less
        //     } else {
        //         std::cmp::Ordering::Equal
        //     }
        // });

        // for format in formats {
        //     println!("Available: {:?}", format);
        // }

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



    

    let mut orb_program = OrbProgram {
        config: OrbConfig {
            max_features: 1 << 14,
            max_matches: 1 << 14,
            image_size: wgpu::Extent3d { 
                width: frame_width, 
                height: frame_height, 
                depth_or_array_layers: 1
            },
            hierarchy_depth: 3
        },
        compute: Compute::new(
            {
                let mut features = wgpu::Features::PUSH_CONSTANTS;

                features |= wgpu::Features::BGRA8UNORM_STORAGE;
                features |= wgpu::Features::TIMESTAMP_QUERY;
                features |= wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES;
                features |= wgpu::Features::CLEAR_TEXTURE;
                
                features
            },
            {
                let mut limits = wgpu::Limits::default();
                limits.max_push_constant_size = 4;
                limits.max_storage_buffers_per_shader_stage = 8;
                limits
            }
            
        ).block_on(),
        storage: Default::default()
    };

    orb_program.init();

    let mut vis = VisualizationProgram {
        compute: orb_program.compute(),
        surface: orb_program.compute().instance.create_surface(&window).unwrap(),
        storage: Default::default()
    };

    vis.init(wgpu::Extent3d {
        width: frame_width,
        height: frame_height,
        depth_or_array_layers: 1
    });

    let mut config = vis
        .surface
        .get_default_config(&orb_program.compute().adapter, frame_width, frame_height)
        .unwrap();
    
    vis.surface.configure(&orb_program.compute().device, &config);

    let mut frame_count: u32 = 0;

    let window = &window;
    let orb_program = &orb_program;

    event_loop.run(move |event, target| {

        if let Event::WindowEvent {
            window_id: _,
            event,
        } = event
        {
            match event {
                WindowEvent::Resized(new_size) => {
                    config.width = new_size.width.max(1);
                    config.height = new_size.height.max(1);
                    vis.surface.configure(&orb_program.compute().device, &config);
                    window.request_redraw();
                },
                WindowEvent::RedrawRequested => {
                    let frame = vis.surface.get_current_texture().unwrap();
                    let view = frame
                        .texture
                        .create_view(&wgpu::TextureViewDescriptor::default());
                    let mut encoder =
                        orb_program.compute()
                            .device
                            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                                label: Some("Draw loop"),
                            });

                    {
                        let new_camera_frame = camera.frame().unwrap();

                        new_camera_frame
                            .decode_image_to_buffer::<RgbAFormat>(&mut frame_buffer)
                            .unwrap();

                        orb_program.write_input_image(&frame_buffer);
                    }

                    orb_program.run(OrbParams {
                        record_keyframe: frame_count == 100,
                    });

                    if frame_count == 100 {
                        println!("Recorded keyframe.");
                    }

                    encoder.copy_texture_to_texture(
                        wgpu::ImageCopyTextureBase {
                            texture: &orb_program.storage().textures["visualization"],
                            mip_level: 0,
                            origin: wgpu::Origin3d::ZERO,
                            aspect: wgpu::TextureAspect::All,
                        },
                        wgpu::ImageCopyTextureBase {
                            texture: &vis.storage().textures["texture"],
                            mip_level: 0,
                            origin: wgpu::Origin3d::ZERO,
                            aspect: wgpu::TextureAspect::All,
                        },
                        orb_program.config.image_size.clone(),
                    );

                    {
                        let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: None,
                            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                view: &view,
                                resolve_target: None,
                                ops: wgpu::Operations {
                                    load: wgpu::LoadOp::Clear(wgpu::Color::GREEN),
                                    store: wgpu::StoreOp::Store,
                                },
                            })],
                            depth_stencil_attachment: None,
                            timestamp_writes: None,
                            occlusion_query_set: None,
                        });
                        rpass.set_pipeline(&vis.storage().render_pipelines["draw_texture"]);
                        rpass.set_bind_group(0, &vis.storage().bind_groups["draw_texture"], &[]);
                        rpass.draw(0..3, 0..1);
                    }

                    orb_program.compute().queue.submit(Some(encoder.finish()));

                    frame.present();

                    window.request_redraw();

                    frame_count += 1;
                },
                WindowEvent::CloseRequested => {
                    target.exit();
                },
                _ => {}
            }
        }
    })
}

fn main() -> Result<(), winit::error::EventLoopError> {
    let event_loop = EventLoop::new().unwrap();
    let window = Window::new(&event_loop).unwrap();
    window.set_title("tinyslam example");
    run(event_loop, Arc::new(window))
}
