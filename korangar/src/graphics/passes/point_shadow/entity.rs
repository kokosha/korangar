use bumpalo::Bump;
use bytemuck::{Pod, Zeroable};
use hashbrown::HashMap;
use wgpu::{
    include_wgsl, BindGroupDescriptor, BindGroupEntry, BindGroupLayout, BindGroupLayoutDescriptor, BindGroupLayoutEntry, BindingResource,
    BindingType, CompareFunction, DepthBiasState, DepthStencilState, Device, FragmentState, MultisampleState, PipelineCompilationOptions,
    PipelineLayoutDescriptor, PrimitiveState, RenderPass, RenderPipeline, RenderPipelineDescriptor, ShaderModuleDescriptor, ShaderStages,
    StencilState, TextureSampleType, TextureView, TextureViewDimension, VertexState,
};

use crate::graphics::passes::{
    BindGroupCount, ColorAttachmentCount, DepthAttachmentCount, Drawer, PointShadowEntityBatchData, PointShadowRenderPassContext,
    RenderPassContext,
};
use crate::graphics::*;

const SHADER: ShaderModuleDescriptor = include_wgsl!("shader/entity.wgsl");
const SHADER_BINDLESS: ShaderModuleDescriptor = include_wgsl!("shader/entity_bindless.wgsl");
const DRAWER_NAME: &str = "point shadow entity";
const INITIAL_INSTRUCTION_SIZE: usize = 256;

#[derive(Copy, Clone, Pod, Zeroable)]
#[repr(C)]
struct InstanceData {
    world: [[f32; 4]; 4],
    texture_position: [f32; 2],
    texture_size: [f32; 2],
    depth_offset: f32,
    curvature: f32,
    mirror: u32,
    texture_index: i32,
}

pub(crate) struct PointShadowEntityDrawer {
    bindless_support: bool,
    solid_pixel_texture: Arc<Texture>,
    instance_data_buffer: Buffer<InstanceData>,
    bind_group_layout: BindGroupLayout,
    bind_group: BindGroup,
    pipeline: RenderPipeline,
    instance_data: Vec<InstanceData>,
    bump: Bump,
    lookup: HashMap<u64, i32>,
}

impl Drawer<{ BindGroupCount::Two }, { ColorAttachmentCount::None }, { DepthAttachmentCount::One }> for PointShadowEntityDrawer {
    type Context = PointShadowRenderPassContext;
    type DrawData<'data> = &'data PointShadowEntityBatchData<'data>;

    fn new(
        capabilities: &Capabilities,
        device: &Device,
        _queue: &Queue,
        global_context: &GlobalContext,
        render_pass_context: &Self::Context,
    ) -> Self {
        let shader_module = if capabilities.supports_bindless() {
            device.create_shader_module(SHADER_BINDLESS)
        } else {
            device.create_shader_module(SHADER)
        };

        let instance_data_buffer = Buffer::with_capacity(
            device,
            format!("{DRAWER_NAME} instance data"),
            BufferUsages::COPY_DST | BufferUsages::STORAGE,
            (size_of::<InstanceData>() * INITIAL_INSTRUCTION_SIZE) as _,
        );

        let bind_group_layout = if capabilities.supports_bindless() {
            device.create_bind_group_layout(&BindGroupLayoutDescriptor {
                label: Some(DRAWER_NAME),
                entries: &[
                    BindGroupLayoutEntry {
                        binding: 0,
                        visibility: ShaderStages::VERTEX_FRAGMENT,
                        ty: BindingType::Buffer {
                            ty: BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: NonZeroU64::new(size_of::<InstanceData>() as _),
                        },
                        count: None,
                    },
                    BindGroupLayoutEntry {
                        binding: 1,
                        visibility: ShaderStages::FRAGMENT,
                        ty: BindingType::Texture {
                            sample_type: TextureSampleType::Float { filterable: true },
                            view_dimension: TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: capabilities.get_max_texture_binding_array_count(),
                    },
                ],
            })
        } else {
            device.create_bind_group_layout(&BindGroupLayoutDescriptor {
                label: Some(DRAWER_NAME),
                entries: &[BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::VERTEX_FRAGMENT,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: NonZeroU64::new(size_of::<InstanceData>() as _),
                    },
                    count: None,
                }],
            })
        };

        let bind_group = if capabilities.supports_bindless() {
            Self::create_bind_group_bindless(device, &bind_group_layout, &instance_data_buffer, &[global_context
                .solid_pixel_texture
                .get_texture_view()])
        } else {
            Self::create_bind_group(device, &bind_group_layout, &instance_data_buffer)
        };

        let pass_bind_group_layouts = Self::Context::bind_group_layout(device);

        let bind_group_layouts: &[&BindGroupLayout] = if capabilities.supports_bindless() {
            &[pass_bind_group_layouts[0], pass_bind_group_layouts[1], &bind_group_layout]
        } else {
            &[
                pass_bind_group_layouts[0],
                pass_bind_group_layouts[1],
                &bind_group_layout,
                Texture::bind_group_layout(device),
            ]
        };

        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some(DRAWER_NAME),
            bind_group_layouts,
            push_constant_ranges: &[],
        });

        let mut constants = std::collections::HashMap::new();
        constants.insert("near_plane".to_string(), NEAR_PLANE as f64);

        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some(DRAWER_NAME),
            layout: Some(&pipeline_layout),
            vertex: VertexState {
                module: &shader_module,
                entry_point: "vs_main",
                compilation_options: PipelineCompilationOptions {
                    constants: &constants,
                    ..Default::default()
                },
                buffers: &[],
            },
            fragment: Some(FragmentState {
                module: &shader_module,
                entry_point: "fs_main",
                compilation_options: PipelineCompilationOptions {
                    constants: &constants,
                    ..Default::default()
                },
                targets: &[],
            }),
            multiview: None,
            primitive: PrimitiveState::default(),
            multisample: MultisampleState::default(),
            depth_stencil: Some(DepthStencilState {
                format: render_pass_context.depth_attachment_output_format()[0],
                depth_write_enabled: true,
                depth_compare: CompareFunction::Less,
                stencil: StencilState::default(),
                bias: DepthBiasState::default(),
            }),
            cache: None,
        });

        Self {
            bindless_support: capabilities.supports_bindless(),
            solid_pixel_texture: global_context.solid_pixel_texture.clone(),
            instance_data_buffer,
            bind_group_layout,
            bind_group,
            pipeline,
            instance_data: Vec::default(),
            bump: Bump::default(),
            lookup: HashMap::default(),
        }
    }

    fn draw(&mut self, pass: &mut RenderPass<'_>, draw_data: Self::DrawData<'_>) {
        let shadow_caster_index = draw_data.pass_data.shadow_caster_index;
        let face_index = draw_data.pass_data.face_index;
        let batch = &draw_data.caster[shadow_caster_index];

        if batch.entity_count[face_index] == 0 {
            return;
        }

        let offset = batch.entity_offset[face_index] as u32;
        let count = batch.entity_count[face_index] as u32;
        let end = offset + count;

        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(2, &self.bind_group, &[]);
        pass.draw(0..6, offset..end);

        if self.bindless_support {
            pass.draw(0..6, offset..end);
        } else {
            let mut current_texture_id = self.solid_pixel_texture.get_id();
            pass.set_bind_group(3, self.solid_pixel_texture.get_bind_group(), &[]);

            for (index, instruction) in draw_data.instructions[offset as usize..end as usize].iter().enumerate() {
                if instruction.texture.get_id() != current_texture_id {
                    current_texture_id = instruction.texture.get_id();
                    pass.set_bind_group(3, instruction.texture.get_bind_group(), &[]);
                }
                let index = offset + index as u32;

                pass.draw(0..6, index..index + 1);
            }
        }
    }
}

impl Prepare for PointShadowEntityDrawer {
    fn prepare(&mut self, device: &Device, instructions: &RenderInstruction) {
        let draw_count = instructions.point_shadow_entities.len();

        if draw_count == 0 {
            return;
        }

        self.instance_data.clear();

        if self.bindless_support {
            self.bump.reset();
            self.lookup.clear();

            let mut texture_views = Vec::with_capacity_in(draw_count, &self.bump);

            for instruction in instructions.point_shadow_entities.iter() {
                let mut texture_index = texture_views.len() as i32;
                let id = instruction.texture.get_texture().global_id().inner();
                let potential_index = self.lookup.get(&id);

                if let Some(potential_index) = potential_index {
                    texture_index = *potential_index;
                } else {
                    self.lookup.insert(id, texture_index);
                    texture_views.push(instruction.texture.get_texture_view());
                }

                self.instance_data.push(InstanceData {
                    world: instruction.world.into(),
                    texture_position: instruction.texture_position.into(),
                    texture_size: instruction.texture_size.into(),
                    depth_offset: instruction.depth_offset,
                    curvature: instruction.curvature,
                    mirror: instruction.mirror as u32,
                    texture_index,
                });

                texture_views.push(instruction.texture.get_texture_view());
            }

            if texture_views.is_empty() {
                texture_views.push(self.solid_pixel_texture.get_texture_view());
            }

            self.instance_data_buffer.reserve(device, self.instance_data.len());
            self.bind_group = Self::create_bind_group_bindless(device, &self.bind_group_layout, &self.instance_data_buffer, &texture_views)
        } else {
            for instruction in instructions.point_shadow_entities.iter() {
                self.instance_data.push(InstanceData {
                    world: instruction.world.into(),
                    texture_position: instruction.texture_position.into(),
                    texture_size: instruction.texture_size.into(),
                    depth_offset: instruction.depth_offset,
                    curvature: instruction.curvature,
                    mirror: instruction.mirror as u32,
                    texture_index: 0,
                });
            }

            self.instance_data_buffer.reserve(device, self.instance_data.len());
            self.bind_group = Self::create_bind_group(device, &self.bind_group_layout, &self.instance_data_buffer)
        }
    }

    fn upload(&mut self, device: &Device, staging_belt: &mut StagingBelt, command_encoder: &mut CommandEncoder) {
        self.instance_data_buffer
            .write(device, staging_belt, command_encoder, &self.instance_data);
    }
}

impl PointShadowEntityDrawer {
    fn create_bind_group_bindless(
        device: &Device,
        bind_group_layout: &BindGroupLayout,
        instance_data_buffer: &Buffer<InstanceData>,
        texture_views: &[&TextureView],
    ) -> BindGroup {
        device.create_bind_group(&BindGroupDescriptor {
            label: Some(DRAWER_NAME),
            layout: bind_group_layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: instance_data_buffer.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::TextureViewArray(texture_views),
                },
            ],
        })
    }

    fn create_bind_group(device: &Device, bind_group_layout: &BindGroupLayout, instance_data_buffer: &Buffer<InstanceData>) -> BindGroup {
        device.create_bind_group(&BindGroupDescriptor {
            label: Some(DRAWER_NAME),
            layout: bind_group_layout,
            entries: &[BindGroupEntry {
                binding: 0,
                resource: instance_data_buffer.as_entire_binding(),
            }],
        })
    }
}