use ne_math::Transform;
/// This is the first iteration renderer it needs to do: 
/// 1) Render separate meshes
/// 2) Render many instances of one mesh
/// 3) Render many seperate meshes
/// 4) Allow for easy user interface integration 
/// 5) Easy switching from camera
/// 6) Mesh loading from .gltf and .obj and maybe fbx files
/// 7) Level saving/loading from .nscene
/// 8) Effective editing of .nscene file...
pub use ne_math::{Vec2, Vec3, Quat, Mat4};
use ne_window::events::{OnWindowResized, OnWindowScaleFactorChanged, AppExit, OnRedrawRequested, 
    OnWindowCloseRequested, OnFileDragAndDrop, OnCursorEntered, OnCursorLeft, OnReceivedCharacter, OnWindowFocused};
use cameras::free_fly_camera;
use ne::{warn, info, trace};
use ne_app::{App, Plugin, Events, ManualEventReader};
#[cfg(feature = "first_frame_time")]
use ne_app::FIRST_FRAME_TIME;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;
use wgpu::{util::DeviceExt, CommandBuffer, CommandEncoder};
use winit::{
    event::{*, self},
    event_loop::{ControlFlow, EventLoop, EventLoopWindowTarget},
    window::{Fullscreen, WindowId}, dpi::PhysicalSize,
};
//export windowbuilder
pub use winit::window::{Window,WindowBuilder};

use crate::{cameras::free_fly_camera::CameraUniform, user_interface::EguiState, mesh::Example};

#[cfg(feature="ui")]
mod user_interface;
mod cameras;
mod resources;
mod texture;
mod render_modules;
mod mesh;

pub mod math;
// pub mod scene;
// use Scene as CurrentScene; //will be used as a resource...

struct CameraCollection {
    pub camera: free_fly_camera::Camera,
    pub projection: free_fly_camera::Projection,
    pub camera_controller: free_fly_camera::CameraController,
    pub camera_uniform: free_fly_camera::CameraUniform,
    pub camera_buffer: wgpu::Buffer,
    pub camera_bind_group: wgpu::BindGroup,
}

struct State {
    surface: wgpu::Surface,
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface_config: wgpu::SurfaceConfiguration,
    size: winit::dpi::PhysicalSize<u32>,

    camera_collection: CameraCollection,
    
    is_right_mouse_pressed: bool,
    #[cfg(feature = "ui")]
    ui_state: user_interface::EguiState,

    mesh1:mesh::Example,
    mesh2:mesh::Example,
}
impl State {
    async fn new(app:&mut App, window: &Window, window_settings: WindowSettings) -> Self {
        //================================================================================================================
        //Window and wgpu initialization
        //================================================================================================================
        // ne::log!("size of struct {} ", std::mem::size_of::<State>());
        let size = window.inner_size();
        // The instance is a handle to our GPU
        warn!("WGPU setup");
        let backend = wgpu::util::backend_bits_from_env().unwrap_or_else(wgpu::Backends::all);
        ne::log!("backend: {:?}", backend);
        let instance = wgpu::Instance::new(backend);
        let surface = unsafe { instance.create_surface(window) };
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                //will use the highest performance gpu.
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .unwrap(); //TODO maybe replace this unwrap with a match?
        warn!("device and queue");
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: None,
                    features: wgpu::Features::empty(),
                    // WebGL doesn't support all of wgpu's features, so if
                    // we're building for the web we'll have to disable some.
                    limits: if cfg!(target_arch = "wasm32") {
                        wgpu::Limits::downlevel_webgl2_defaults()
                    } else {
                        wgpu::Limits::default()
                    },
                },
                // Some(&std::path::Path::new("trace")), // Trace path
                None, // Trace path
            )
            .await
            .unwrap();
        warn!("Surface");
        let surface_format= surface.get_supported_formats(&adapter)[0];
        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width,
            height: size.height,
            present_mode: window_settings.present_mode,
        };
        surface.configure(&device, &surface_config);
        let texture_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            multisampled: false,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
                label: Some("texture_bind_group_layout"),
            });
        //================================================================================================================
        //camera buffer
        //================================================================================================================
        //TODO accessibility: camera location
        let camera = free_fly_camera::Camera::new(Vec3::new(0.0, 4.0, 10.0), -90.0, 0.0);
        let projection =
        free_fly_camera::Projection::new(surface_config.width, surface_config.height, 45.0, 0.1, 100.0);
        //TODO accessibility 
        let camera_controller = free_fly_camera::CameraController::new(4.0, 0.8);
        
        let mut camera_uniform = CameraUniform::new();
        camera_uniform.update_view_proj(&camera, &projection);

        let camera_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Camera Buffer"),
            contents: bytemuck::cast_slice(&[camera_uniform]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let camera_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
                label: Some("camera_bind_group_layout"),
            });
        let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &camera_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_buffer.as_entire_binding(),
            }],
            label: Some("camera_bind_group"),
        });
        //================================================================================================================
        //Scene loading
        //================================================================================================================
        warn!("Load scene");
        //load meshes
        // let mesh1 = Mesh::load_mesh_from_path("obj", Transform::default());
        // let mesh2 = Mesh::load_mesh_from_path("obj", Transform::default());
        let mesh1 = Example::init(
            &surface_config, &adapter, &device, &queue,Vec3{x: 1.0, y: 0.0, z: 0.0 }
        );
        let mesh2 = Example::init(
            &surface_config, &adapter, &device, &queue,Vec3{x: -1.0, y: 0.0, z: 0.0 }
        );
        // let scene_loader = app.world.remove_resource::<SceneLoader>();
        // let scene = match scene_loader
        // {
        //     Some(scene_loader) => {
        //         //initialize once we have models vector
        //         CurrentScene::new(
        //             scene_loader,
        //             &device,
        //             &queue,
        //             &texture_bind_group_layout).await
        //     }
        //     None => {
        //         //initialize once we have models vector
        //         CurrentScene::new(
        //             SceneLoader::default(),
        //             &device,
        //             &queue,
        //             &texture_bind_group_layout).await
        //     }
        // };
        // app.world.insert_resource::<CurrentScene>(scene);
        // let mut scene = app.world.get_resource_mut::<CurrentScene>().unwrap();

        //lol this is weird because these values are moved onto the struct? will the reference understand that??
        #[cfg(feature="ui")]
        let ui_state = EguiState::new(window, &device, &surface_format  /*,  &queue, &surface_config, &adapter, &surface, */);
        Self {
            surface,
            device,
            queue,
            surface_config,
            size,
            camera_collection: CameraCollection {
                camera,
                projection,
                camera_controller,
                camera_buffer,
                camera_bind_group,
                camera_uniform,
            },
            is_right_mouse_pressed: false,
            #[cfg(feature="ui")]
            ui_state: ui_state,

            mesh1,
            mesh2,
        }
    }

    fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        if new_size.width > 0 && new_size.height > 0 {
            self.camera_collection.projection.resize(new_size.width, new_size.height);
            self.size = new_size;
            self.surface_config.width = new_size.width;
            self.surface_config.height = new_size.height;
            self.surface.configure(&self.device, &self.surface_config);
            // self.depth_texture =
            //     texture::Texture::create_depth_texture(&self.device, &self.surface_config, "depth_texture");
        }
    }
    fn input(&mut self, event: &WindowEvent) -> bool {
        match event {
            WindowEvent::KeyboardInput {
                input:
                    KeyboardInput {
                        virtual_keycode: Some(key),
                        state,
                        ..
                    },
                ..
            } => self.camera_collection.camera_controller.process_keyboard(*key, *state),
            WindowEvent::MouseWheel { delta, .. } => {
                self.camera_collection.camera_controller.process_scroll(delta);
                true
            }
            WindowEvent::MouseInput {
                button: MouseButton::Right,
                state,
                ..
            } => {
                self.is_right_mouse_pressed = *state == ElementState::Pressed;
                true
            }
            _ => false,
        }

    }
    //updates camera, can be cleaner/faster/moved into camera.rs... maybe
    fn update(&mut self, dt:f32) {
        self.camera_collection.camera_controller.update_camera(&mut self.camera_collection.camera, dt);
        self.camera_collection.camera_uniform.update_view_proj(&self.camera_collection.camera, &self.camera_collection.projection);
        self.queue.write_buffer(
            &self.camera_collection.camera_buffer,
            0,
            bytemuck::cast_slice(&[self.camera_collection.camera_uniform]),
        );
    }
    //TODO double&triple buffer
    //TODO isolate from state and measure performance..?
    //TODO is window:&Window bad?

    fn create_encoder(&self) -> CommandEncoder {
        self
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Render Encoder"),
        })
    }
    fn render(&mut self, app:&mut App, window:&winit::window::Window) -> Result<(), wgpu::SurfaceError>  {
        let output_frame = self.surface.get_current_texture()?;
        let output_view = output_frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut cmd_buffers = Vec::<CommandBuffer>::new();
        
        //new encoder
        let mut encoder = self.create_encoder();
        //clear frame and set background color.
        {
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.1,
                            g: 0.2,
                            b: 0.3,
                            a: 1.0,
                        }),
                        store: true,
                    },
                    view: &output_view,
                })],
                depth_stencil_attachment: None,
            });
        }
        cmd_buffers.push(encoder.finish());

        //TODO how to make these meshes share buffers..? Obviously the same vertex/index buffer with a slightly different model buffer.        
        cmd_buffers.push(
        self.mesh1.render(&output_view, &self.device));
        //problem this one clears the one before... solution: nothing clears the renderer clear itself first.
        cmd_buffers.push(
        self.mesh2.render(&output_view, &self.device));

        //new encoder
        let mut encoder = self.create_encoder();
        // UI RENDERING! WIll be rendered on top of the previous output!!!
        #[cfg(feature="ui")]
        {
            self.ui_state.update_time();

            // Begin to draw the UI frame.
            self.ui_state.begin_frame();
            // Draw the demo application.
            self.ui_state.draw_ui();

            // End the UI frame. We could now handle the output and draw the UI with the backend.
            let full_output = self.ui_state.end_frame(window);
            let paint_jobs = self.ui_state.platform.context().tessellate(full_output.shapes);

            // Upload all resources for the GPU.
            let screen_descriptor = user_interface::ui_render_pass::ScreenDescriptor {
                physical_width: self.surface_config.width,
                physical_height: self.surface_config.height,
                scale_factor: window.scale_factor() as f32,
            };
            let tdelta: egui::TexturesDelta = full_output.textures_delta;
            self.ui_state.render_pass
                .add_textures(&self.device, &self.queue, &tdelta)
                .expect("add texture ok");
            self.ui_state.render_pass.update_buffers(&self.device, &self.queue, &paint_jobs, &screen_descriptor);

            // Record all render passes.
            self.ui_state.render_pass
                .execute(
                    &mut encoder,
                    &output_view,
                    &paint_jobs,
                    &screen_descriptor,
                    None,
                )
                .unwrap();

                cmd_buffers.push(encoder.finish());

            // remove ui data for some reason..?
            self.ui_state.render_pass
            .remove_textures(tdelta)
            .expect("remove texture ok");
        }
        // the number of submit() calls should be limited to a few per frame (e.g. 1-5).
        self.queue.submit(cmd_buffers);
        output_frame.present();

        Ok(()) 

/*         
{        cmd_buffers.push(self.mesh1.draw(self.create_encoder()))}
{        cmd_buffers.push(self.mesh2.draw(self.create_encoder()))}

        //new encoder
        //perpare meshes
        // let a = app.world.get_resource::<CurrentScene>().unwrap().get_models();
        // for model in a.iter()
        // {
        //     let mut encoder = self
        //     .device
        //     .create_command_encoder(&wgpu::CommandEncoderDescriptor {
        //     label: Some("Model Render Encoder")});

            // render_pass.set_pipeline(&self.render_pipeline);
            //BufferSlice { buffer: Buffer { context: Context { type: "Native" }, id: Buffer { id: (1, 1, Vulkan), error_sink: Mutex { data: ErrorSink } }, map_context: Mutex { data: MapContext { total_size: 64, initial_range: 0..0, sub_ranges: [] } }, usage: VERTEX }, offset: 0, size: None }
            // let mesh = &model.mesh;
            // {
            //     let material = &model.materials[mesh.material];
            //     render_pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
            //     render_pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
            //     render_pass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            //     render_pass.set_bind_group(0, &material.bind_group, &[]);
            //     render_pass.set_bind_group(1, &self.camera_bind_group, &[]);
            //     render_pass.draw_indexed(0..mesh.num_elements, 0, 0..2);
            //     // model::DrawModel::draw_model_instanced(&mut render_pass, model, 0..1, &self.camera_bind_group);
            // }
            // cmd_buffers.push(encoder.finish());
            //something like this would be faster tho.. but we will have some kind of other models that can
            //only be drawn as instances: InstancedModels. For e.g. trees in a forest.
            // render_pass.draw_model_instanced(
            //     &self.models[0],
            //     0..self.instances.len() as u32,
            //     &self.camera_bind_group,
            // );
        // }
        
        
        //this is not how it works... I don't think
        Ok(()) 
    } */
}
}
fn main_loop(app: App) {
    //is this async implementation any good?
    pollster::block_on(init_renderer(app));
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen(start))]
async fn init_renderer(mut app: App) {
    ne::debug!("init_renderer");

    let event_loop = EventLoop::new();

    let win_settings =  app.world.get_resource::<WindowSettings>()
        .cloned().unwrap_or_default();
    let window = create_window(&win_settings, &event_loop);
    let mut state = State::new(&mut app, &window, win_settings).await;

    trace!("pre event_loop.run");

    //benchmark values.
    #[cfg(feature = "first_frame_time")]
    let mut once_benchmark = true;
    let mut last_render_time = instant::Instant::now();
    let mut frame_count:u64 = 1;
    //exit window event reader
    let mut app_exit_event_reader = ManualEventReader::<AppExit>::default();

    #[cfg(feature = "print_fps")]
    let mut fpsd = ne_bench::fpsdata::FPSData::default();

    let event_handler = 
    move |event: Event<()>,
        _: &EventLoopWindowTarget<()>, //not sure what to do with event_loop: &EventLoopWindowTarget
        control_flow: &mut ControlFlow| {
            //maybe move this after if !state.input(event) {
            #[cfg(feature="ui")]
            let ui_event = &event;

        match event {
            event::Event::MainEventsCleared => {
                window.request_redraw();
                //are these supposed to be here?
                app.update();
            },
            event::Event::WindowEvent {
                //HOW DOES THIS EVENT GET HERE???
                    ref event,
                    window_id,
                } if window_id == window.id() => {
                    let world = app.world.cell();
                    if !state.input(event) {
                        #[cfg(feature="ui")]
                        state.ui_state.handle_event(ui_event);
                        match event {
                            WindowEvent::CloseRequested => {
                                let mut window_close_requested_events =
                                    world.resource_mut::
                                        <Events<OnWindowCloseRequested>>();
                                window_close_requested_events.send(
                                    OnWindowCloseRequested { id: window_id });
                            }
                            WindowEvent::KeyboardInput {
                                input:
                                    KeyboardInput {
                                        state: ElementState::Pressed,
                                        virtual_keycode: Some(VirtualKeyCode::Escape),
                                        ..
                                    },
                                ..
                            } => *control_flow = ControlFlow::Exit,
                            WindowEvent::Resized(physical_size) => {
    
                                //TODO MOVE TASK: decouple window and renderer
                                state.resize(*physical_size);
                                
                                let mut resize_events
                                = world.resource_mut::<Events<OnWindowResized>>();
                                resize_events.send(OnWindowResized {
                                    id: window_id,
                                    width:  window.inner_size().width as f32,
                                    height: window.inner_size().height as f32,
                                });
                            }
                            WindowEvent::ScaleFactorChanged { scale_factor, new_inner_size } => {
                                state.resize(**new_inner_size);

                                let mut scale_event = world.resource_mut
                                ::<Events<OnWindowScaleFactorChanged>>();
                                scale_event.send(OnWindowScaleFactorChanged 
                                    { id: window_id, scale_factor: *scale_factor })

                            }
                            WindowEvent::Focused(focused)
                            => {
                                let mut focused_events = 
                                    world.resource_mut::<Events<OnWindowFocused>>();
                                focused_events.send(OnWindowFocused {
                                    id: window_id,
                                    //TODO why does this have to be dereferenced?
                                    focused: *focused });
                            }
                            WindowEvent::HoveredFile(path_buf) => {
                                let mut events = world.resource_mut::<Events<OnFileDragAndDrop>>();
                                events.send(OnFileDragAndDrop::HoveredFile {
                                    id: window_id,
                                    //TODO TEST
                                    path_buf: path_buf.to_path_buf(),
                                });
                            }
                            WindowEvent::DroppedFile(path_buf) => {
                                let mut events = world.resource_mut::<Events<OnFileDragAndDrop>>();
                                events.send(OnFileDragAndDrop::DroppedFile {
                                    id: window_id,
                                    path_buf: path_buf.to_path_buf(),
                                });
                            }
                        //TODO implement multiple windows before implementing this one.
                        WindowEvent::CursorMoved { position, .. }
                        => {
                        }
                        WindowEvent::CursorEntered{.. /* device id needed? */ } 
                        => {
                            let mut cursor_entered_events =
                                world.resource_mut::<Events<OnCursorEntered>>();
                            cursor_entered_events.send(OnCursorEntered { id: window_id });
                        }
                        WindowEvent::CursorLeft { .. } => {
                            let mut cursor_left_event 
                            = world.resource_mut::<Events<OnCursorLeft>>();
                            cursor_left_event.send(OnCursorLeft { id: window_id });
                        }
                        //TODO TEST
                        WindowEvent::ReceivedCharacter(c) => {
                            let mut char_input_events =
                                world.resource_mut::<Events<OnReceivedCharacter>>();
    
                            char_input_events.send(OnReceivedCharacter {
                                id: window_id,
                                //TODO this dereference hits performance? Measure this
                                char: *c,
                            });
                        }
                         _ => {}
                        }
                    }
                }
            event::Event::DeviceEvent {
                    event: DeviceEvent::MouseMotion{ delta, },
                    .. // We're not using device_id currently
                } => if state.is_right_mouse_pressed {
                    state.camera_collection.camera_controller.process_mouse(delta.0, delta.1);
                    window.set_cursor_visible(false);
                    window.set_cursor_position(winit::dpi::PhysicalPosition::new(window.inner_size().width/2, window.inner_size().height/2));
                } else {
                    window.set_cursor_visible(true);
                }
            event::Event::RedrawRequested(window_id) if window_id == window.id() => {
                    //hope this gets optimized somehow
                    #[cfg(feature = "first_frame_time")]
                    if once_benchmark //do once
                    {
                        unsafe{
                            FIRST_FRAME_TIME = Some(instant::Instant::now());
                        }
                        once_benchmark=false;
                    }
                    let now = instant::Instant::now();
                    let delta_time = (now - last_render_time).as_secs_f32();
                    last_render_time = now;
    
                    #[cfg(feature = "print_fps")]
                    {
                        //TODO move maybe
                        let fps = 1.0/delta_time;
                        // a little messy
                        frame_count+=1;
                        unsafe
                        {
                            let time_passed = (now - FIRST_FRAME_TIME.unwrap()).as_secs_f32();
                            let average_fps = frame_count as f32/time_passed;
                            
                            ne::log!("fps:{:<14}fps | avg:{:<14}fps | 1%LOW:{:<10}fps",fps,average_fps, fpsd.get_lowest(fps));
                        }
                    }
                    state.update(delta_time);
    
                    match state.render(&mut app, &window ) {
                        Ok(_) => {}
                        // Reconfigure the surface if it's lost or outdated
                        Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                            state.resize(state.size)
                        }
                        // The system is out of memory, we should probably quit
                        Err(wgpu::SurfaceError::OutOfMemory) => *control_flow = ControlFlow::Exit,
                        // We're ignoring timeouts
                        Err(wgpu::SurfaceError::Timeout) => warn!("Surface timeout"),
                    }

                    //TODO remove..?
                    let world = app.world.cell();
                    let mut frame_events = world.resource_mut::<Events<OnRedrawRequested>>();
                    frame_events.send(OnRedrawRequested {});
                }
                event::Event::RedrawEventsCleared => {
                    //measure
                    if let Some(app_exit_events) = 
                        app.world.get_resource::<Events<AppExit>>() {
                        if app_exit_event_reader.iter(app_exit_events).last().is_some() {
                            *control_flow = ControlFlow::Exit;
                        }
                    }
                }
                _ => 
                {
    
                }
            }
    };
    ne::log!("event loop run");
    run(event_loop, event_handler);
}

fn run<F>(event_loop: EventLoop<()>, event_handler: F) -> !
where
    F: 'static + FnMut(Event<'_, ()>, &EventLoopWindowTarget<()>, &mut ControlFlow),
{
    event_loop.run(event_handler)
}

// #[derive(Default)]
///sets runner using .set_runner()
pub struct RenderPlugin;
impl Plugin for RenderPlugin {
    fn setup(&self, app: &mut App) {
        ne::debug!("setup RenderPlugin");
        app.add_plugin(ne_window::WindowEventPlugin)

        // .init_resource::<Windows>()
        .set_runner(main_loop);
    }
}
/// ================================================================================================
/// Window functionality
/// ================================================================================================

pub use wgpu::PresentMode;
/// Describes the information needed for creating a window.
///
/// This should be set up before adding the [`WindowPlugin`](crate::WindowPlugin).
/// Most of these settings can also later be configured through the [`Window`](crate::Window) resource.
///
/// See [`examples/window/window_settings.rs`] for usage.
///
/// [`examples/window/window_settings.rs`]: https://github.com/bevyengine/bevy/blob/latest/examples/window/window_settings.rs
#[derive(Debug, Clone)]
pub struct WindowSettings {
    /// Sets the title that displays on the window top bar, on the system task bar and other OS specific places.
    ///
    /// ## Platform-specific
    /// - Web: Unsupported.
    pub title: String,
    /// The requested logical width of the window's client area.
    /// May vary from the physical width due to different pixel density on different monitors.
    pub width: f32,
    /// The requested logical height of the window's client area.
    /// May vary from the physical height due to different pixel density on different monitors.
    pub height: f32,
    /// ## Platform-specific
    /// - iOS / Android / Web: Unsupported.
    /// - macOS X: Not working as expected.
    /// - Windows 11: Not working as expected
    /// macOS X transparent works with winit out of the box, so this issue might be related to: <https://github.com/gfx-rs/wgpu/issues/687>
    /// Windows 11 is related to <https://github.com/rust-windowing/winit/issues/2082>
    /// Sets whether the background of the window should be transparent.
    pub transparent: bool,
    /// - iOS / Android / Web: Unsupported.
    pub resizable: bool,
    /// Sets whether the window should have borders and bars.
    pub decorations: bool,
    /// Sets whether the cursor is visible when the window has focus.
    //TODO
    pub cursor_visible: bool,
    /// Sets whether the window locks the cursor inside its borders when the window has focus.
    //TODO
    pub cursor_locked: bool,
    /// Whether or not to fit the canvas element's size to its parent element's size.
    ///
    /// **Warning**: this will not behave as expected for parents that set their size according to the size of their
    /// children. This creates a "feedback loop" that will result in the canvas growing on each resize. When using this
    /// feature, ensure the parent's size is not affected by its children.
    ///
    /// This value has no effect on non-web platforms.
    //TODO
    pub fit_canvas_to_parent: bool,
    /// Controls when a frame is presented to the screen.
    #[doc(alias = "vsync")]
    /// The window's [`PresentMode`].
    /// Used to select whether or not VSync is used
    pub present_mode: wgpu::PresentMode,
    /// The position on the screen that the window will be placed at.
    pub position: WindowPosition,
    /// Sets minimum and maximum resize limits.
    pub resize_constraints: WindowResizeConstraints,
    /// Overrides the window's ratio of physical pixels to logical pixels.
    ///
    /// If there are some scaling problems on X11 try to set this option to `Some(1.0)`.
    pub scale_factor_override: Option<f64>,
    /// Sets whether the window is resizable.
    /// ## Platform-specific
    /// Sets the [`WindowMode`](crate::WindowMode).
    pub window_mode: WindowMode,
    /// The "html canvas" element selector.
    /// If set, this selector will be used to find a matching html canvas element,
    /// rather than creating a new one.
    /// Uses the [CSS selector format](https://developer.mozilla.org/en-US/docs/Web/API/Document/querySelector).
    /// This value has no effect on non-web platforms.
    pub canvas: Option<String>,
}
impl Default for WindowSettings {
    fn default() -> Self {
        WindowSettings {
            title: "app".to_string(),
            width: 1280.,
            height: 720.,

            resizable: true,
            decorations: true,
            cursor_locked: false,
            cursor_visible: true,
            transparent: false,
            fit_canvas_to_parent: false,
            present_mode: wgpu::PresentMode::Immediate,
                        
            //TODO THESE NEED TO BE ENABLED ON STARTUP
            position: WindowPosition::Automatic,
            resize_constraints: WindowResizeConstraints::default(),
            scale_factor_override: None,

            window_mode: WindowMode::Windowed,
            canvas: None,
        }
    }
}

//window
pub fn get_fitting_videomode(
    monitor: &winit::monitor::MonitorHandle,
    width: u32,
    height: u32,
) -> winit::monitor::VideoMode {
    let mut modes = monitor.video_modes().collect::<Vec<_>>();

    fn abs_diff(a: u32, b: u32) -> u32 {
        if a > b {
            return a - b;
        }
        b - a
    }
    //does this work..?
    modes.sort_by(|a, b| {
        use std::cmp::Ordering::*;
        match abs_diff(a.size().width, width).cmp(&abs_diff(b.size().width, width)) {
            Equal => {
                match abs_diff(a.size().height, height).cmp(&abs_diff(b.size().height, height)) {
                    Equal => b.refresh_rate_millihertz().cmp(&a.refresh_rate_millihertz()),
                    default => default,
                }
            }
            default => default,
        }
    });

    modes.first().unwrap().clone()
}

fn create_window(win_settings: &WindowSettings, event_loop: &EventLoop<()>) -> Window
{
    let mut wind = winit::window::WindowBuilder::new()
    .with_title(win_settings.title.clone())
    .with_inner_size(PhysicalSize::new(win_settings.width, win_settings.height))
    .with_transparent(win_settings.transparent)
    .with_resizable(win_settings.resizable)
    .with_decorations(win_settings.decorations);
    match win_settings.window_mode
    {
        //incomplete 
        WindowMode::Windowed => {},
        WindowMode::BorderlessFullscreen => {
            // let mut monitor_index = 0; //todo
            let monitor = event_loop
            .available_monitors()
            .next()
            .expect("no monitor found!");
            let fullscreen = Some(Fullscreen::Borderless(Some(monitor.clone())));
            info!("Setting mode: {:?}", fullscreen);
            wind = wind.with_fullscreen(fullscreen);
        },
        WindowMode::SizedFullscreen => todo!(),
        WindowMode::Fullscreen => {
            // let mut monitor_index = 0; //todo
            let monitor = event_loop
            .available_monitors()
            .next()
            .expect("no monitor found!");
            let fullscreen = Some(Fullscreen::Exclusive(get_fitting_videomode(
                &monitor,
                win_settings.width as u32,
                win_settings.height as u32,
            )),);
            info!("Setting mode: {:?}", fullscreen);
            wind = wind.with_fullscreen(fullscreen);
        },
    }

    //TODO ...
    // match (win_settings.mode)
    // {
    //     WindowMode::Windowed => (),
    //     WindowMode::BorderlessFullscreen => wind.with_fullscreen(),
    //     WindowMode::SizedFullscreen => todo!(),
    //     WindowMode::Fullscreen => wind.with_fullscreen(),
    // }

    wind
    .build(&event_loop)
    .unwrap()
}
/// Defines the way a window is displayed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowMode {
    /// Creates a window that uses the given size.
    Windowed,
    /// Creates a borderless window that uses the full size of the screen.
    BorderlessFullscreen,
    /// Creates a fullscreen window that will render at desktop resolution.
    ///
    /// The app will use the closest supported size from the given size and scale it to fit the screen.
    SizedFullscreen,
    /// Creates a fullscreen window that uses the maximum supported size.
    Fullscreen,
}

/// Defines where window should be placed at on creation.
#[derive(Debug, Clone, Copy)]
pub enum WindowPosition {
    /// Position will be set by the window manager
    Automatic,
    /// Window will be centered on the selected monitor
    ///
    /// Note that this does not account for window decorations.
    Centered(MonitorSelection),
    /// The window's top-left corner will be placed at the specified position (in pixels)
    ///
    /// (0,0) represents top-left corner of screen space.
    At(Vec2),
}

/// Defines which monitor to use.
#[derive(Debug, Clone, Copy)]
pub enum MonitorSelection {
    /// Uses current monitor of the window.
    Current,
    /// Uses primary monitor of the system.
    Primary,
    /// Uses monitor with the specified index.
    Number(usize),
}

//Needed?
/// The size limits on a window.
///
/// These values are measured in logical pixels, so the user's
/// scale factor does affect the size limits on the window.
/// Please note that if the window is resizable, then when the window is
/// maximized it may have a size outside of these limits...
#[derive(Debug, Clone, Copy)]
pub struct WindowResizeConstraints {
    pub min_width: f32,
    pub min_height: f32,
    pub max_width: f32,
    pub max_height: f32,
}

impl Default for WindowResizeConstraints {
    fn default() -> Self {
        Self {
            min_width: 180.,
            min_height: 120.,
            max_width: f32::INFINITY,
            max_height: f32::INFINITY,
        }
    }
}

impl WindowResizeConstraints {
    #[must_use]
    pub fn check_constraints(&self) -> Self {
        let WindowResizeConstraints {
            mut min_width,
            mut min_height,
            mut max_width,
            mut max_height,
        } = self;
        min_width = min_width.max(1.);
        min_height = min_height.max(1.);
        if max_width < min_width {
            warn!(
                "The given maximum width {} is smaller than the minimum width {}",
                max_width, min_width
            );
            max_width = min_width;
        }
        if max_height < min_height {
            warn!(
                "The given maximum height {} is smaller than the minimum height {}",
                max_height, min_height
            );
            max_height = min_height;
        }
        WindowResizeConstraints {
            min_width,
            min_height,
            max_width,
            max_height,
        }
    }
}
