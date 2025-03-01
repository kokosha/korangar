use cgmath::{Matrix4, Point3, SquareMatrix, Transform as PointTransform, VectorSpace};
use derive_new::new;
use korangar_interface::elements::PrototypeElement;
use ragnarok_formats::model::{RotationKeyframeData, ScaleKeyframeData, TranslationKeyframeData};
use ragnarok_formats::version::InternalVersion;
use ragnarok_packets::ClientTick;

use crate::graphics::ModelInstruction;
use crate::world::Camera;

const CLIENT_TICK_PER_SECOND: u32 = 5000;

#[derive(PrototypeElement, new)]
pub struct Node {
    pub version: InternalVersion,
    #[hidden_element]
    pub transform_matrix: Matrix4<f32>,
    #[hidden_element]
    pub parent_transform_matrix: Matrix4<f32>,
    #[hidden_element]
    pub centroid: Point3<f32>,
    pub transparent: bool,
    pub vertex_offset: usize,
    pub vertex_count: usize,
    pub child_nodes: Vec<Node>,
    pub frames_per_second: f32,
    pub animation_length: u32,
    pub scale_keyframes: Vec<ScaleKeyframeData>,
    pub translation_keyframes: Vec<TranslationKeyframeData>,
    pub rotation_keyframes: Vec<RotationKeyframeData>,
}

impl Node {
    fn scale_animation_matrix(&self, client_tick: ClientTick) -> Matrix4<f32> {
        let timestamp = match self.version.equals_or_above(2, 2) {
            false => client_tick.0,
            true => client_tick.0 / (CLIENT_TICK_PER_SECOND / self.frames_per_second as u32),
        };
        let last_step = self.scale_keyframes.last().unwrap();
        let animation_length = last_step.frame.min(self.animation_length);
        let animation_tick = timestamp % animation_length;

        let animation_tick_f32 = match self.version.equals_or_above(2, 2) {
            false => (client_tick.0 % animation_length) as f32,
            true => {
                (client_tick.0 % (animation_length * (CLIENT_TICK_PER_SECOND / self.frames_per_second as u32))) as f32
                    / (CLIENT_TICK_PER_SECOND / self.frames_per_second as u32) as f32
            }
        };

        let last_keyframe_index = self
            .scale_keyframes
            .binary_search_by(|keyframe| keyframe.frame.cmp(&animation_tick))
            .unwrap_or_else(|keyframe_index| {
                // Err(i) returns the index where the searched element could be inserted to
                // retain the sort order. This means, that we haven't reached a
                // new keyframe yet and need to use the previous keyframe, hence
                // the saturating sub.
                keyframe_index.saturating_sub(1)
            });

        let last_step = &self.scale_keyframes[last_keyframe_index];
        let next_step = &self.scale_keyframes[(last_keyframe_index + 1) % self.scale_keyframes.len()];

        let total = next_step.frame.saturating_sub(last_step.frame);
        let offset = (animation_tick_f32 - (last_step.frame as f32)).min(total as f32);

        let animation_elapsed = (1.0 / total as f32) * offset as f32;
        let current_scale = last_step.scale.lerp(next_step.scale, animation_elapsed);

        Matrix4::from_nonuniform_scale(current_scale.x, current_scale.y, current_scale.z)
    }

    fn translation_animation_matrix(&self, client_tick: ClientTick) -> Matrix4<f32> {
        let timestamp = match self.version.equals_or_above(2, 2) {
            false => client_tick.0,
            true => client_tick.0 / (CLIENT_TICK_PER_SECOND / self.frames_per_second as u32),
        };
        let last_step = self.translation_keyframes.last().unwrap();
        let animation_length = last_step.frame.min(self.animation_length);
        let animation_tick = timestamp % animation_length;

        let animation_tick_f32 = match self.version.equals_or_above(2, 2) {
            false => (client_tick.0 % animation_length) as f32,
            true => {
                (client_tick.0 % (animation_length * (CLIENT_TICK_PER_SECOND / self.frames_per_second as u32))) as f32
                    / (CLIENT_TICK_PER_SECOND / self.frames_per_second as u32) as f32
            }
        };

        let last_keyframe_index = self
            .translation_keyframes
            .binary_search_by(|keyframe| keyframe.frame.cmp(&animation_tick))
            .unwrap_or_else(|keyframe_index| {
                // Err(i) returns the index where the searched element could be inserted to
                // retain the sort order. This means, that we haven't reached a
                // new keyframe yet and need to use the previous keyframe, hence
                // the saturating sub.
                keyframe_index.saturating_sub(1)
            });

        let last_step = &self.translation_keyframes[last_keyframe_index];
        let next_step = &self.translation_keyframes[(last_keyframe_index + 1) % self.translation_keyframes.len()];

        let total = next_step.frame.saturating_sub(last_step.frame);
        let offset = (animation_tick_f32 - (last_step.frame as f32)).min(total as f32);

        let animation_elapsed = (1.0 / total as f32) * offset as f32;
        let current_translation = last_step.translation.lerp(next_step.translation, animation_elapsed);

        Matrix4::from_translation(current_translation)
    }

    fn rotation_animation_matrix(&self, client_tick: ClientTick) -> Matrix4<f32> {
        let timestamp = match self.version.equals_or_above(2, 2) {
            false => client_tick.0,
            true => client_tick.0 / (CLIENT_TICK_PER_SECOND / self.frames_per_second as u32),
        };
        let last_step = self.rotation_keyframes.last().unwrap();
        let animation_length = last_step.frame.min(self.animation_length);
        let animation_tick = timestamp % animation_length;

        let animation_tick_f32 = match self.version.equals_or_above(2, 2) {
            false => (client_tick.0 % animation_length) as f32,
            true => {
                (client_tick.0 % (animation_length * (CLIENT_TICK_PER_SECOND / self.frames_per_second as u32))) as f32
                    / (CLIENT_TICK_PER_SECOND / self.frames_per_second as u32) as f32
            }
        };
        let last_keyframe_index = self
            .rotation_keyframes
            .binary_search_by(|keyframe| keyframe.frame.cmp(&animation_tick))
            .unwrap_or_else(|keyframe_index| {
                // Err(i) returns the index where the searched element could be inserted to
                // retain the sort order. This means, that we haven't reached a
                // new keyframe yet and need to use the previous keyframe, hence
                // the saturating sub.
                keyframe_index.saturating_sub(1)
            });

        let last_step = &self.rotation_keyframes[last_keyframe_index];
        let next_step = &self.rotation_keyframes[(last_keyframe_index + 1) % self.rotation_keyframes.len()];

        let total = next_step.frame.saturating_sub(last_step.frame);
        let offset = (animation_tick_f32 - (last_step.frame as f32)).min(total as f32);

        let animation_elapsed = (1.0 / total as f32) * offset;
        let current_rotation = last_step.quaternions.nlerp(next_step.quaternions, animation_elapsed);

        current_rotation.into()
    }

    pub fn world_matrix(&self, client_tick: ClientTick, parent_matrix: &Matrix4<f32>, is_static: bool) -> Matrix4<f32> {
        match is_static {
            true => parent_matrix * self.transform_matrix,
            false => {
                let animation_scale_matrix = match self.scale_keyframes.is_empty() {
                    true => Matrix4::identity(),
                    false => self.scale_animation_matrix(client_tick),
                };
                let animation_translation_matrix = match self.translation_keyframes.is_empty() {
                    true => match self.version.smaller(2, 2) {
                        true => Matrix4::<f32>::identity(),
                        false => self.transform_matrix * self.parent_transform_matrix.invert().unwrap(),
                    },
                    false => self.translation_animation_matrix(client_tick),
                };
                let animation_rotation_matrix = match self.rotation_keyframes.is_empty() {
                    true => Matrix4::identity(),
                    false => self.rotation_animation_matrix(client_tick),
                };

                if self.version.smaller(2, 2) {
                    parent_matrix * self.transform_matrix * animation_rotation_matrix * animation_scale_matrix
                } else {
                    parent_matrix * animation_translation_matrix * animation_rotation_matrix * animation_scale_matrix
                }
            }
        }
    }

    pub fn render_geometry(
        &self,
        instructions: &mut Vec<ModelInstruction>,
        client_tick: ClientTick,
        camera: &dyn Camera,
        node_index: usize,
        parent_matrix: &Matrix4<f32>,
        is_static: bool,
    ) {
        // Some models have multiple nodes with the same position. This can lead so
        // z-fighting, when we sort the model instructions later with an unstable,
        // non-allocating sort. To remove this z-fighting, we add a very small offset to
        // the nodes, so that they always have the same order from the same view
        // perspective.
        let draw_order_offset = (node_index as f32) * 1.1920929e-4_f32;

        let model_matrix = self.world_matrix(client_tick, parent_matrix, is_static);
        let position = model_matrix.transform_point(self.centroid);
        let distance = camera.distance_to(position) + draw_order_offset;

        instructions.push(ModelInstruction {
            model_matrix,
            vertex_offset: self.vertex_offset,
            vertex_count: self.vertex_count,
            distance,
            transparent: self.transparent,
        });

        // When the render_geometry is set as static, the model matrix
        // is already pre-calculated.
        // When the render_geometry is set as dynamic, the model matrix
        // needs to be calculated recursively, because the nodes from the model change
        // positions due to animation motion.
        let parent_matrix = match is_static {
            true => parent_matrix,
            false => &model_matrix,
        };

        self.child_nodes
            .iter()
            .enumerate()
            .for_each(|(node_index, node)| node.render_geometry(instructions, client_tick, camera, node_index, &parent_matrix, is_static));
    }
}
