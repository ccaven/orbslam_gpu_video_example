/*
DONE:
 - Camera streaming to winit window
 - Vertex/fragment shader drawing the contents of a storage texture

TODO:
 - Connect ORBFeatureExtractor pipeline
 - Implement proper BRIEF feature descriptors
    - Step 1: Slightly blur image texture
        - Two passes, one for blur x and one for blur y
    - Step 2: Integral image
        - log2(max(width, height)) passes
    - Step 3: Detect FAST corners
        - Corner detection pass + however many merge passes are needed
    - Step 4: BRIEF descriptors
        - One pass, write to pre-allocated texture, no need for atomics
    - Step 5: Feature matching
        - Brute force for now

*/

use std::{borrow::Cow, num::NonZeroU64, sync::Arc};

use pollster::FutureExt;
use wgpu::util::{BufferInitDescriptor, DeviceExt};
use winit::{
    event::{Event, WindowEvent},
    event_loop::EventLoop,
    window::Window,
};

use nokhwa::{pixel_format::RgbAFormat, utils::{CameraFormat, FrameFormat, RequestedFormat, Resolution}, Camera};

use orbslam_gpu::orb_2::{OrbConfig, OrbParams, OrbProgram};

use tiny_wgpu::Compute;

struct VisualizationProgram<'a> {
    pub surface: wgpu::Surface<'a>,
    pub shader: wgpu::ShaderModule,
    pub pipeline: wgpu::RenderPipeline,
    pub bind_group: wgpu::BindGroup,
    pub output_image_buffer: wgpu::Buffer,
    pub output_image_size: wgpu::Extent3d,
    pub base_texture: wgpu::Texture
}

impl VisualizationProgram<'_> {
    pub fn new(compute: &Compute, output_image_size: wgpu::Extent3d, window: Arc<Window>) -> Self {
        let surface = compute.instance.create_surface(window).unwrap();

        let shader = compute
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("Simple Draw Texture Shader"),
                source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!("draw_texture.wgsl"))),
            });

        let bind_group_layout = compute.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer { 
                        ty: wgpu::BufferBindingType::Storage { read_only: true }, 
                        has_dynamic_offset: false, 
                        min_binding_size: Some(NonZeroU64::new(4).unwrap())
                    },
                    count: None
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: Some(NonZeroU64::new(8).unwrap())
                    },
                    count: None
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: Some(NonZeroU64::new(8).unwrap())
                    },
                    count: None
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture { 
                        sample_type: wgpu::TextureSampleType::Float { filterable: false }, 
                        view_dimension: wgpu::TextureViewDimension::D2, 
                        multisampled: false
                    },
                    count: None
                }
            ]
        });

        let pipeline_layout = compute
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: None,
                // TODO: Fill this out to match layout of incoming texture
                bind_group_layouts: &[&bind_group_layout],
                push_constant_ranges: &[],
            });

        let swapchain_capabilities = surface.get_capabilities(&compute.adapter);
        let swapchain_format = swapchain_capabilities.formats[0];

        let output_image_buffer = compute.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: (output_image_size.width * output_image_size.height * 4) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false
        });

        let pipeline = compute.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                targets: &[Some(swapchain_format.into())],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        let texture_size_buffer = compute.device.create_buffer_init(&BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&[ output_image_size.width, output_image_size.height ]),
            usage: wgpu::BufferUsages::UNIFORM
        });

        let window_size_buffer = compute.device.create_buffer_init(&BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&[ output_image_size.width, output_image_size.height ]),
            usage: wgpu::BufferUsages::UNIFORM
        });

        let base_texture = compute.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Base texture"),
            size: output_image_size,
            sample_count: 1,
            mip_level_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[]
        });

        let base_texture_view = base_texture.create_view(&wgpu::TextureViewDescriptor::default());


        let bind_group = compute.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: output_image_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: texture_size_buffer.as_entire_binding()
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: window_size_buffer.as_entire_binding()
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&base_texture_view)
                }
            ]
        });

        Self {
            shader,
            surface,
            pipeline,
            bind_group,
            output_image_buffer,
            output_image_size,
            base_texture
        }
    }
}

async fn run(
    event_loop: EventLoop<()>,
    window: Arc<Window>,
) -> Result<(), winit::error::EventLoopError> {

    let mut camera = {
        let mut camera = Camera::new(
            nokhwa::utils::CameraIndex::Index(0),
            RequestedFormat::new::<nokhwa::pixel_format::RgbAFormat>(
                //nokhwa::utils::RequestedFormatType::HighestFrameRate(30)
                // nokhwa::utils::RequestedFormatType::Exact(
                //     CameraFormat::new(
                //         //Resolution::new(320, 240),
                //         Resolution::new(800, 600),
                //         FrameFormat::MJPEG,
                //         30
                //     )
                // )
                nokhwa::utils::RequestedFormatType::AbsoluteHighestResolution
            ),
        )
        .unwrap();

        println!("Frame rate: {}", camera.refresh_camera_format().unwrap().frame_rate());

        let mut formats = camera.compatible_camera_formats().unwrap();

        formats.sort_by(|a, b| {
            if a.frame_rate() > b.frame_rate() {
                std::cmp::Ordering::Greater
            } else if a.frame_rate() < b.frame_rate() {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Equal
            }
        });

        for format in formats {
            println!("Available: {:?}", format);
        }

        camera.open_stream().expect("Could not open stream.");

        camera
    };

    let (frame_width, frame_height) = {
        let frame = camera.frame().unwrap();
        let resolution = frame.resolution();

        (resolution.width(), resolution.height())
        // (640, 480)
    };

    // Create Vec<u8> and fill with zeros
    // This will hold the decoded image
    let mut frame_buffer = vec![0u8; (frame_width * frame_height * 4) as usize];

    let mut window_size = window.inner_size();

    window_size.width = frame_width;
    window_size.height = frame_height;

    let _ = window.request_inner_size(window_size);

    let compute = Compute::init().await;

    let output_image_size = wgpu::Extent3d {
        width: window_size.width,
        height: window_size.height,
        depth_or_array_layers: 1
    };

    let vis = VisualizationProgram::new(
        &compute, 
        output_image_size.clone(), 
        window.clone()
    );

    let orb_program = OrbProgram::init(OrbConfig {
        max_features: 8192,
        image_size: vis.output_image_size
    }, compute.clone());

    let mut config = vis.surface
        .get_default_config(&compute.adapter, window_size.width, window_size.height)
        .unwrap();
    vis.surface.configure(&compute.device, &config);

    let window = &window;

    let mut frame_count: u32 = 0;

    event_loop.run(move |event, target| {
        let _ = (&compute, &vis);

        if let Event::WindowEvent {
            window_id: _,
            event,
        } = event
        {
            match event {
                WindowEvent::Resized(new_size) => {
                    config.width = new_size.width.max(1);
                    config.height = new_size.height.max(1);
                    vis.surface.configure(&compute.device, &config);
                    window.request_redraw();
                }
                WindowEvent::RedrawRequested => {
                    let frame = vis.surface.get_current_texture().unwrap();
                    let view = frame
                        .texture
                        .create_view(&wgpu::TextureViewDescriptor::default());
                    let mut encoder = compute.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                        label: Some("Draw loop"),
                    });

                    // Decode the image on the CPU and write the decoded buffer to the GPU
                    // TODO: Try to use VulkanVideo to stream directly to GPU
                    // Or gstreamer with Vulkan integration
                    
                    let new_camera_frame = camera.frame().unwrap();
                    new_camera_frame.decode_image_to_buffer::<RgbAFormat>(&mut frame_buffer).unwrap();

                    compute.queue.write_texture(
                        wgpu::ImageCopyTexture {
                            texture: &orb_program.program.textures["input_image"],
                            mip_level: 0,
                            origin: wgpu::Origin3d::ZERO,
                            aspect: wgpu::TextureAspect::All
                        },
                        &frame_buffer,
                        wgpu::ImageDataLayout {
                            offset: 0,
                            bytes_per_row: (4 * frame_width).into(),
                            rows_per_image: None
                        },
                        output_image_size
                    );

                    orb_program.run(OrbParams {
                        record_keyframe: frame_count == 100
                    });

                    if frame_count == 100 {
                        println!("Recorded keyframe.");
                    }

                    encoder.copy_texture_to_texture(
                        wgpu::ImageCopyTextureBase { 
                            texture: &orb_program.program.textures["visualization"], 
                            mip_level: 0, 
                            origin: wgpu::Origin3d::ZERO, 
                            aspect: wgpu::TextureAspect::All
                        },
                        wgpu::ImageCopyTextureBase { 
                            texture: &vis.base_texture, 
                            mip_level: 0, 
                            origin: wgpu::Origin3d::ZERO, 
                            aspect: wgpu::TextureAspect::All
                        },
                        orb_program.config.image_size.clone()
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
                        rpass.set_pipeline(&vis.pipeline);
                        rpass.set_bind_group(0, &vis.bind_group, &[]);
                        rpass.draw(0..3, 0..1);
                    }

                    compute.queue.submit(Some(encoder.finish()));

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
    run(event_loop, Arc::new(window)).block_on()
}
