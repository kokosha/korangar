use std::sync::Arc;

use cgmath::{Array, Matrix4, Point3, Vector2, Zero};
use korangar_interface::elements::PrototypeElement;
use ragnarok_packets::EntityId;

use crate::graphics::{Camera, Color, EntityInstruction, Texture};
use crate::loaders::{Actions, AnimationState};
use crate::world::EntityType;

#[derive(Clone, PrototypeElement)]
pub struct AnimationData {
    pub animations: Vec<Animation>,
    pub animation_pairs: Vec<AnimationPair>,
    pub delays: Vec<f32>,
    #[hidden_element]
    pub entity_type: EntityType,
    #[hidden_element]
    pub texture: Arc<Texture>,
}

#[derive(Clone, PrototypeElement)]
pub struct AnimationPair {
    pub actions: Arc<Actions>,
    pub texture_sub_allocations: Vec<TextureSubAllocation>,
}

#[derive(Clone, PrototypeElement)]
pub struct TextureSubAllocation {
    pub position: Vector2<f32>,
    pub size: Vector2<f32>,
}

#[derive(Clone, PrototypeElement)]
pub struct Animation {
    #[hidden_element]
    pub frames: Vec<AnimationFrame>,
}

#[derive(Clone)]
pub struct AnimationFrame {
    pub offset: Vector2<i32>,
    pub top_left: Vector2<i32>,
    pub size: Vector2<i32>,
    /// Used for the final shift
    pub remove_offset: Vector2<i32>,
    pub frame_parts: Vec<AnimationFramePart>,
}

#[derive(Clone)]
pub struct AnimationFramePart {
    pub animation_index: usize,
    pub sprite_number: usize,
    pub offset: Vector2<i32>,
    pub size: Vector2<i32>,
    pub mirror: bool,
    pub angle: f32,
    pub color: Color,
    pub affine_matrix: Matrix4<f32>,
}

impl Default for AnimationFramePart {
    fn default() -> AnimationFramePart {
        AnimationFramePart {
            animation_index: usize::MAX,
            sprite_number: usize::MAX,
            offset: Vector2::<i32>::zero(),
            size: Vector2::<i32>::zero(),
            mirror: Default::default(),
            angle: Default::default(),
            color: Default::default(),
            affine_matrix: Matrix4::<f32>::zero(),
        }
    }
}

impl AnimationData {
    pub fn render(
        &self,
        instructions: &mut Vec<EntityInstruction>,
        camera: &dyn Camera,
        entity_id: EntityId,
        entity_position: Point3<f32>,
        animation_state: &AnimationState,
        head_direction: usize,
    ) {
        let camera_direction = camera.camera_direction();
        let direction = (camera_direction + head_direction) % 8;
        let aa = animation_state.action * 8 + direction;
        let delay = self.delays[aa % self.delays.len()];
        let animation = &self.animations[aa % self.animations.len()];

        let factor = animation_state
            .factor
            .map(|factor| delay * (factor / 5.0))
            .unwrap_or_else(|| delay * 50.0);

        let frame_time = animation_state
            .duration
            .map(|duration| animation_state.time * animation.frames.len() as u32 / duration)
            .unwrap_or_else(|| (animation_state.time as f32 / factor) as u32);

        // TODO: Work out how to avoid losing digits when casting time to an f32. When
        //       fixed remove set_start_time in MouseCursor.
        let time = frame_time as usize % animation.frames.len();
        let mut frame = &animation.frames[time];

        // Remove Doridori animation from Player
        if self.entity_type == EntityType::Player && animation_state.action == 0 {
            frame = &animation.frames[0];
        }

        for (index, frame_part) in frame.frame_parts.iter().enumerate() {
            let animation_index = frame_part.animation_index;
            let sprite_number = frame_part.sprite_number;

            // The constant 10.0 is a magic scale factor of an image.
            // The vertex position is calculated from the center of image, so we need to
            // add half of the height.
            let position = Vector2::new(
                animation.frames[0].offset.x as f32,
                animation.frames[0].offset.y as f32 + ((animation.frames[time].size.y - 1) / 2) as f32,
            ) / 10.0;

            let origin = Point3::new(-position.x, position.y, 0.0);
            let scale = Vector2::from_value(0.7);
            let size = Vector2::new(frame.size.x as f32 * scale.x / 10.0, frame.size.y as f32 * scale.y / 10.0);

            let world_matrix = camera.billboard_matrix(entity_position, origin, size);
            let affine_matrix = frame_part.affine_matrix;

            let texture_sub_allocation = &self.animation_pairs[animation_index].texture_sub_allocations[sprite_number];
            let (depth_offset, curvature) = camera.calculate_depth_offset_and_curvature(&world_matrix, scale.x, scale.y);

            instructions.push(EntityInstruction {
                world: world_matrix,
                frame_part_transform: affine_matrix,
                texture_position: texture_sub_allocation.position,
                texture_size: texture_sub_allocation.size,
                depth_offset,
                extra_depth_offset: 0.001 * index as f32,
                curvature,
                angle: frame_part.angle,
                color: frame_part.color,
                mirror: frame_part.mirror,
                entity_id,
                texture: self.texture.clone(),
            });
        }
    }
}
