// import heaven yep
use bevy::core_pipeline::node::MAIN_PASS_DRIVER;
use bevy::core_pipeline::Transparent2d;
use bevy::prelude::*;
use bevy::render::camera::{ActiveCamera, Camera2d, CameraPlugin};
use bevy::render::render_graph::{NodeRunError, RenderGraphContext, SlotInfo, SlotType, SlotValue};
use bevy::render::render_phase::RenderPhase;
use bevy::render::render_resource::{
    BindGroupDescriptor, BindGroupEntry, BindGroupLayout, BindGroupLayoutDescriptor,
    BindGroupLayoutEntry, BindingType, BlendState, Buffer, BufferAddress, BufferBindingType,
    BufferUsages, ColorTargetState, IndexFormat, LoadOp, Operations, PipelineLayoutDescriptor,
    PolygonMode, PrimitiveState, PrimitiveTopology, RawFragmentState, RawRenderPipelineDescriptor,
    RawVertexBufferLayout, RawVertexState, RenderPassColorAttachment, RenderPassDescriptor,
    RenderPipeline, ShaderModuleDescriptor, ShaderSource, ShaderStages, TextureFormat,
    VertexAttribute, VertexFormat, VertexStepMode,
};
use bevy::render::renderer::{RenderContext, RenderDevice};
use bevy::render::view::{ExtractedView, ExtractedWindows, ViewTarget};
use bevy::render::{
    render_graph::{Node, RenderGraph},
    render_resource::BufferInitDescriptor,
    RenderApp,
};

// some stuff we use to help create the post_process graph, mainly the labels
pub mod post_process_graph {
    pub const NAME: &str = "post_processing_graph";

    pub mod input {
        pub const VIEW_ENTITY: &str = "view_entity";
        pub const RESOLUTION: &str = "resolution_buffer";
    }
    pub mod node {
        pub const POST_PROCESS_DRIVER: &str = "post_processing_driver";
        pub const VIGNETTE_NODE: &str = "vignette_node";
    }
}

struct VignetteNode {
    // will get the resource with these components using this query
    query_target:
        QueryState<(&'static RenderPhase<Transparent2d>, &'static ViewTarget), With<ExtractedView>>,
}

// resolution resource we pass into the shader
#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct Resolution {
    x: f32,
    y: f32,
}

// defines some constants we use to define our vignette node
impl VignetteNode {
    pub const IN_VIEW: &'static str = "view";
    pub const IN_RESOLUTION: &'static str = "resolution_buffer";

    pub fn new(world: &mut World) -> Self {
        Self {
            query_target: QueryState::new(world),
        }
    }
}

struct PostProcessingDriver;

// booo
struct VignetteData {
    vertex_buffer: Buffer,
    index_buffer: Buffer,
    vignette_pipeline: RenderPipeline,
    indices_length: u32,
    vignette_bind_group_layout: BindGroupLayout,
}

impl Node for VignetteNode {
    fn input(&self) -> Vec<SlotInfo> {
        // these are our inputs
        // the camera view
        // and za resolution!
        vec![
            SlotInfo::new(VignetteNode::IN_VIEW, SlotType::Entity),
            SlotInfo::new(VignetteNode::IN_RESOLUTION, SlotType::Buffer),
        ]
    }

    fn update(&mut self, world: &mut World) {
        // update the query to have all the data everytime
        // we run the vignette node
        self.query_target.update_archetypes(world);
    }

    fn run(
        &self,
        graph: &mut RenderGraphContext,
        render_context: &mut RenderContext,
        world: &World,
    ) -> Result<(), NodeRunError> {
        // get the values which are going from the input of the sub graph
        // and put them into our vignette_node so we can use them!
        let view_entity = graph.get_input_entity(VignetteNode::IN_VIEW).unwrap();
        let resolution_buffer = graph.get_input_buffer(VignetteNode::IN_RESOLUTION).unwrap();

        // get the vignette_data created earlier
        let vignette_data = world.get_resource::<VignetteData>().unwrap();

        // create the resource/bind group to pass into
        // the shader from the resolution_buffer
        let vignette_bind_group =
            render_context
                .render_device
                .create_bind_group(&BindGroupDescriptor {
                    label: Some("vignette_bind_group"),
                    layout: &vignette_data.vignette_bind_group_layout,
                    entries: &[BindGroupEntry {
                        binding: 0,
                        resource: resolution_buffer.as_entire_binding(),
                    }],
                });

        // get our target
        let (_transparent_phase, target) =
            self.query_target.get_manual(world, view_entity).unwrap();

        // create the render pass with the cameras view as our target
        // also keeps the previously drawn data and just overlays the
        // vignette ontop!
        let mut vignette_pass =
            render_context
                .command_encoder
                .begin_render_pass(&RenderPassDescriptor {
                    label: Some("Vignette Pass"),
                    color_attachments: &[
                        // This is what [[location(0)]] in the fragment shader targets
                        RenderPassColorAttachment {
                            view: &target.view,
                            resolve_target: None,
                            ops: Operations {
                                load: LoadOp::Load,
                                store: true,
                            },
                        },
                    ],
                    depth_stencil_attachment: None,
                });

        // set stuff for the render pass
        // and draw our vignette
        vignette_pass.set_pipeline(&vignette_data.vignette_pipeline);
        vignette_pass.set_bind_group(0, &vignette_bind_group, &[]);
        vignette_pass.set_vertex_buffer(0, *vignette_data.vertex_buffer.slice(..));
        vignette_pass.set_index_buffer(*vignette_data.index_buffer.slice(..), IndexFormat::Uint16); // 1.
        vignette_pass.draw_indexed(0..vignette_data.indices_length, 0, 0..1); // 2.

        // ok sure
        Result::Ok(())
    }
}

impl Node for PostProcessingDriver {
    fn run(
        &self,
        graph: &mut RenderGraphContext,
        render_context: &mut RenderContext,
        world: &World,
    ) -> Result<(), NodeRunError> {
        // get the camera or panic
        let camera_2d = match world.resource::<ActiveCamera<Camera2d>>().get() {
            None => {
                panic!("failed to get 2d orthographic camera in post processing driver node run")
            }
            Some(camera) => camera,
        };

        // get all windows in program
        let windows = world.get_resource::<ExtractedWindows>().unwrap();

        // default resolution value
        let mut resolution = Resolution { x: 1280., y: 720. };

        // get our main windows resolution
        for (window_id, window) in &windows.windows {
            if window_id.is_primary() {
                resolution.x = window.physical_width as f32;
                resolution.y = window.physical_height as f32;
            }
        }

        // create the uniform buffer to pass into the bind group
        let resolution_buffer =
            render_context
                .render_device
                .create_buffer_with_data(&BufferInitDescriptor {
                    label: Some("post_processing_render_buffer"),
                    contents: bytemuck::cast_slice(&[resolution]),
                    usage: BufferUsages::UNIFORM,
                });

        // run the post processing graph
        graph
            .run_sub_graph(
                post_process_graph::NAME,
                vec![
                    SlotValue::Entity(camera_2d),
                    SlotValue::Buffer(resolution_buffer),
                ],
            )
            .unwrap();
        Ok(())
    }
}

impl FromWorld for VignetteData {
    fn from_world(world: &mut World) -> Self {
        // get the render device
        let render_device = world.get_resource_mut::<RenderDevice>().unwrap();

        // vertices which basically are at every corner of the screen
        let vertices: &[f32; 12] = &[-1., -1., 0., -1., 1., 0., 1., 1., 0., 1., -1., 0.];

        // form two triangles which form a rectangle that takes up the entire screen
        let indices: &[u16] = &[0, 2, 1, 0, 3, 2];

        // create the vertex_buffer from the vertices array
        let vertex_buffer = render_device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("vignette_vertex_buffer"),
            contents: bytemuck::cast_slice(vertices),
            usage: BufferUsages::VERTEX,
        });

        // create the index buffer from the indices array
        let index_buffer = render_device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("vignette_index_buffer"),
            contents: bytemuck::cast_slice(indices),
            usage: BufferUsages::INDEX,
        });

        // load in the vignette shader
        let shader = render_device.create_shader_module(&ShaderModuleDescriptor {
            label: Some("vignette_shader"),
            source: ShaderSource::Wgsl(include_str!("shaders/vignette.wgsl").into()),
        });

        // layout for the vertex buffer
        let vertex_buffer_layout = RawVertexBufferLayout {
            array_stride: std::mem::size_of::<[f32; 3]>() as BufferAddress,
            step_mode: VertexStepMode::Vertex,
            attributes: &[VertexAttribute {
                offset: 0,
                shader_location: 0,
                format: VertexFormat::Float32x3,
            }],
        };

        // bind group = resource that we can parse into the shader for it to use
        // this bind group passes in a uniform buffer which contains the Resolution struct
        let vignette_bind_group_layout =
            render_device.create_bind_group_layout(&BindGroupLayoutDescriptor {
                label: Some("vignette_bind_group_layout"),
                entries: &[BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });

        // layout for the render pipeline
        let vignette_pipeline_layout =
            render_device.create_pipeline_layout(&PipelineLayoutDescriptor {
                label: Some("vignette_pipeline_layout"),
                bind_group_layouts: &[&vignette_bind_group_layout],
                push_constant_ranges: &[],
            });

        // create the render_pipeline from the previous values
        let vignette_pipeline =
            render_device.create_render_pipeline(&RawRenderPipelineDescriptor {
                label: Some("vignette_pipeline"),
                layout: Some(&vignette_pipeline_layout),
                primitive: PrimitiveState {
                    topology: PrimitiveTopology::TriangleList,
                    strip_index_format: None,
                    front_face: Default::default(),
                    cull_mode: None,
                    unclipped_depth: false,
                    polygon_mode: PolygonMode::Fill,
                    conservative: false,
                },

                vertex: RawVertexState {
                    module: &shader,
                    entry_point: "vs_main",
                    buffers: &[vertex_buffer_layout],
                },

                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(RawFragmentState {
                    module: &shader,
                    entry_point: "fs_main",
                    targets: &[ColorTargetState {
                        format: TextureFormat::Bgra8UnormSrgb,
                        blend: Some(BlendState::ALPHA_BLENDING),
                        write_mask: Default::default(),
                    }],
                }),
                multiview: None,
            });

        // length of the index buffer
        let indices_length = indices.len() as u32;

        // create our VignetteData resource for node to use
        VignetteData {
            vertex_buffer,
            index_buffer,
            vignette_pipeline,
            indices_length,
            vignette_bind_group_layout,
        }
    }
}

pub fn post_processing(app: &mut App) {
    let render_app = app.sub_app_mut(RenderApp);
    render_app.init_resource::<VignetteData>();

    // create our vignette node using the render_apps world
    let vignette_node = VignetteNode::new(&mut render_app.world);

    // get render_apps render_graph
    let mut render_graph = render_app.world.get_resource_mut::<RenderGraph>().unwrap();

    // create a post_processing_graph subgraph
    let mut post_processing_graph = bevy::render::render_graph::RenderGraph::default();

    // add the inputs of camera entity & resolution buffer to the post_processing_graph
    let input_node_id = post_processing_graph.set_input(vec![
        SlotInfo::new(post_process_graph::input::VIEW_ENTITY, SlotType::Entity),
        SlotInfo::new(post_process_graph::input::RESOLUTION, SlotType::Buffer),
    ]);

    // Add the vignette_node to the post_processing_graph
    post_processing_graph.add_node(post_process_graph::node::VIGNETTE_NODE, vignette_node);

    // Add camera transfer between input and vignette_node
    post_processing_graph
        .add_slot_edge(
            input_node_id,
            post_process_graph::input::VIEW_ENTITY,
            post_process_graph::node::VIGNETTE_NODE,
            VignetteNode::IN_VIEW,
        )
        .unwrap();

    // Add resolution buffer transfer to vignette_node
    post_processing_graph
        .add_slot_edge(
            input_node_id,
            post_process_graph::input::RESOLUTION,
            post_process_graph::node::VIGNETTE_NODE,
            VignetteNode::IN_RESOLUTION,
        )
        .unwrap();

    // add our post_processing_driver which will run the graph
    render_graph.add_sub_graph(post_process_graph::NAME, post_processing_graph);
    render_graph.add_node(
        post_process_graph::node::POST_PROCESS_DRIVER,
        PostProcessingDriver,
    );

    // have it be run after the main_pass_driver so it happens after the main draws & therefore
    // it will be ontop!
    render_graph
        .add_node_edge(
            MAIN_PASS_DRIVER,
            post_process_graph::node::POST_PROCESS_DRIVER,
        )
        .unwrap();
}
