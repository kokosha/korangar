use std::cmp::{max, min};
use std::collections::HashMap;
use std::sync::Arc;

use cgmath::Vector2;
use derive_new::new;
use korangar_interface::elements::PrototypeElement;
use num::Zero;
use wgpu::{Device, Extent3d, Queue, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages};

use super::error::LoadError;
use crate::graphics::Texture;
use crate::loaders::{ActionLoader, Actions, AnimationState, Sprite, SpriteLoader};
use crate::{Color, EntityType};

// TODO: use cache later, the memory will be increase with this hashmap, until
// the program is out of memory.
#[derive(new)]
pub struct AnimationLoader {
    device: Arc<Device>,
    queue: Arc<Queue>,
    #[new(default)]
    // The string will be type of entity
    // 0_{body_id}_{head_id}
    // 1_{monster_id}
    // 2_{npc_id}
    cache: HashMap<String, AnimationData>,
}

impl AnimationLoader {
    pub fn load(
        &mut self,
        sprite_loader: &mut SpriteLoader,
        action_loader: &mut ActionLoader,
        entity_filename: Vec<String>,
        entity_type: EntityType,
    ) -> Result<AnimationData, LoadError> {
        // Create animation pair with sprite and action
        let vec: Vec<AnimationPair> = entity_filename
            .iter()
            .map(|file_path| AnimationPair {
                sprites: sprite_loader.get(&format!("{file_path}.spr")).unwrap(),
                actions: action_loader.get(&format!("{file_path}.act")).unwrap(),
            })
            .collect();

        // The sprite is stored as rgba in animation_pair.sprites.rgba_images or
        // animation_pair.sprites.palette_images
        // The sprite is stored as texture in animation_pair.sprites.textures

        // For each animation, we collect all the framepart need to generate the frame
        let mut animations_list: Vec<Vec<Vec<Frame>>> = Vec::new();
        let mut animation_index: usize = 0;
        let mut action_index: usize = 0;
        let mut motion_index: usize = 0;

        // Each animation pair has the sprites and actions, we iterate over the
        // animation pairs
        // Each entity has several actions and the actions is composed of several
        // motion, in each motion contains several pictures that we try to
        // merge.
        for animation_pair in vec.iter() {
            action_index = 0;
            let mut animation_frames: Vec<Vec<Frame>> = Vec::new();
            for action in animation_pair.actions.actions.iter() {
                motion_index = 0;
                let mut action_frames: Vec<Frame> = Vec::new();
                for motion in action.motions.iter() {
                    let mut motion_frames: Vec<Frame> = Vec::new();
                    if motion.sprite_clip_count == 0 {
                        continue;
                    }
                    for position in 0..motion.sprite_clip_count {
                        let frame_part: FramePart;
                        let pos = position as usize;
                        let sprite_number = motion.sprite_clips[pos].sprite_number;
                        if sprite_number == -1 {
                            continue;
                        }
                        let texture_size = animation_pair.sprites.textures[sprite_number as usize].get_size();
                        let mut height = texture_size.height;
                        let mut width = texture_size.width;
                        // Apply color filter in the image
                        let color = match motion.sprite_clips[pos].color {
                            Some(color) => {
                                let alpha = ((((color >> 24) & 0xFF) as u8) as f32 / 255.0) as f32;
                                let blue = ((((color >> 16) & 0xFF) as u8) as f32 / 255.0) as f32;
                                let green = ((((color >> 8) & 0xFF) as u8) as f32 / 255.0) as f32;
                                let red = ((((color) & 0xFF) as u8) as f32 / 255.0) as f32;

                                Color { red, green, blue, alpha }
                            }
                            None => Color {
                                red: 0.0,
                                green: 0.0,
                                blue: 0.0,
                                alpha: 0.0,
                            },
                        };

                        // Scale the image
                        // Try to match the first type of zoom, if doesn't match find the second method
                        let zoom = match motion.sprite_clips[pos].zoom {
                            Some(value) => (value, value).into(),
                            None => match motion.sprite_clips[pos].zoom2 {
                                Some(value) => value,
                                None => (1.0, 1.0).into(),
                            },
                        };
                        if zoom != (1.0, 1.0).into() {
                            width = (width as f32 * zoom.x).floor() as u32;
                            height = (height as f32 * zoom.y).floor() as u32;
                        }
                        // Get the image rotation
                        let angle = match motion.sprite_clips[pos].angle {
                            Some(value) => value as f32 / 360.0 * 2.0 * std::f32::consts::PI,
                            None => 0.0,
                        };
                        // Get the offset and if the image is mirrored
                        let mut offset = motion.sprite_clips[pos].position.map(|component| component);
                        let mirror = motion.sprite_clips[pos].mirror_on != 0;

                        // This is hardcoded for head in player for attach_point
                        // animation_index == 0 is head
                        // animation_index == 1 is body
                        let has_attach_point = match motion.attach_point_count {
                            Some(value) => value == 1,
                            None => false,
                        };
                        if entity_type == EntityType::Player && has_attach_point && animation_index == 1 {
                            let parent_animation_pair = &vec[0];
                            let parent_action = &parent_animation_pair.actions.actions[action_index];
                            let parent_motion = &parent_action.motions[motion_index];
                            let parent_attach_point = parent_motion.attach_points[0].position;
                            let attach_point = motion.attach_points[0].position;
                            let new_offset = -attach_point + parent_attach_point;
                            offset += new_offset;
                        }
                        let size = Vector2::new(width as i32, height as i32);

                        frame_part = FramePart {
                            animation_index: animation_index as i32,
                            sprite_number,
                            size,
                            upleft: Vector2::zero(),
                            offset,
                            mirror,
                            angle,
                            color,
                        };
                        let frame = Frame {
                            size,
                            upleft: Vector2::zero(),
                            offset,
                            remove_offset: Vector2::zero(),
                            frameparts: vec![frame_part],
                        };
                        motion_frames.push(frame);
                    }
                    if motion_frames.len() == 1 {
                        action_frames.push(motion_frames[0].clone());
                    } else {
                        let frame = Frame::merge_frame_part(&mut motion_frames);
                        action_frames.push(frame);
                    }
                    motion_index += 1;
                }
                animation_frames.push(action_frames);
                action_index += 1;
            }
            animations_list.push(animation_frames);
            animation_index += 1;
        }
        let action_size = vec[0].actions.actions.len();
        let animation_pair_size = vec.len();

        let mut animations: Vec<Animation> = Vec::new();

        // Generate for each action, the several motions that it have and create a
        // offset of retangles.
        for action_index in 0..action_size {
            let motion_size = vec[0].actions.actions[action_index].motions.len();
            let mut frames: Vec<Frame> = Vec::new();
            for motion_index in 0..motion_size {
                let mut generate: Vec<Frame> = Vec::new();

                for animation_pair_index in 0..animation_pair_size {
                    if animations_list[animation_pair_index].len() <= action_index {
                        continue;
                    }
                    if animations_list[animation_pair_index][action_index].len() <= motion_index {
                        continue;
                    }
                    generate.push(animations_list[animation_pair_index][action_index][motion_index].clone());
                }
                let frame = Frame::merge_frame_part(&mut generate);

                frames.push(frame);
            }

            // The problem is that the offset is not in the correct proportion and
            // it is difficult to operate in 3D dimension.
            // To solve this primarly, we created images of the same size and same offset
            // This code resize the sprites in the same size and same offset
            // Find the max_width and the min and max offset
            let mut max_width = 0;
            let mut max_height = 0;
            let mut min_offset_x = i32::MAX;
            let mut min_offset_y = i32::MAX;
            let mut max_offset_x = 0;
            let mut max_offset_y = 0;
            frames.iter_mut().for_each(|frame| {
                max_width = max(max_width, frame.size.x);
                max_height = max(max_height, frame.size.y);
                min_offset_x = min(min_offset_x, frame.offset.x);
                min_offset_y = min(min_offset_y, frame.offset.y);
                max_offset_x = max(max_offset_x, frame.offset.x);
                max_offset_y = max(max_offset_y, frame.offset.y);
            });
            // The constant 2 is to avoid miscalculation by 1.
            max_width += max_offset_x - min_offset_x + 2;
            max_height += max_offset_y - min_offset_y + 2;

            // Shift every frame by  max_offset_x - min_offset_x + 2
            // As the bottom part and left part will not be in the offset.
            frames.iter_mut().for_each(|frame| {
                let new_width = max_width;
                let new_height = max_height;
                for framepart in frame.frameparts.iter_mut() {
                    framepart.offset.x += max_offset_x - min_offset_x + 2;
                    framepart.offset.y += max_offset_y - min_offset_y + 2;
                }
                frame.offset.x = min_offset_x;
                frame.offset.y = min_offset_y;
                frame.offset.x += max_offset_x - min_offset_x + 2;
                frame.offset.y += max_offset_y - min_offset_y + 2;
                frame.size = Vector2::new(new_width, new_height);
                frame.remove_offset.x = max_offset_x - min_offset_x + 2;
                frame.remove_offset.y = max_offset_y - min_offset_y + 2;
            });
            animations.push(Animation { frames });
        }
        let animation_data = AnimationData {
            delays: (&vec[0]).actions.delays.clone(),
            animation_pair: vec,
            animations,
            entity_type,
        };
        let hash = match entity_type {
            EntityType::Player => format!("0_{}_{}", entity_filename[0], entity_filename[1]),
            EntityType::Monster => format!("1_{}", entity_filename[0]),
            EntityType::Npc => format!("2_{}", entity_filename[0]),
            _ => format!("3"),
        };
        self.cache.insert(hash.clone(), animation_data.clone());
        Ok(animation_data)
    }

    pub fn get(
        &mut self,
        sprite_loader: &mut SpriteLoader,
        action_loader: &mut ActionLoader,
        entity_filename: Vec<String>,
        entity_type: EntityType,
    ) -> Result<AnimationData, LoadError> {
        let hash = match entity_type {
            EntityType::Player => format!("0_{}_{}", entity_filename[0], entity_filename[1]),
            EntityType::Monster => format!("1_{}", entity_filename[0]),
            EntityType::Npc => format!("2_{}", entity_filename[0]),
            _ => format!("3"),
        };

        match self.cache.get(&hash) {
            Some(animation_data) => Ok(animation_data.clone()),
            None => self.load(sprite_loader, action_loader, entity_filename, entity_type),
        }
    }
}

#[derive(Clone)]
pub struct FramePart {
    pub animation_index: i32,
    pub sprite_number: i32,
    pub offset: Vector2<i32>,
    pub upleft: Vector2<i32>, // Is only used for internal calculation
    pub size: Vector2<i32>,
    pub mirror: bool,
    pub angle: f32,
    pub color: Color,
}

#[derive(Clone)]
pub struct Frame {
    pub offset: Vector2<i32>,
    pub upleft: Vector2<i32>,
    pub size: Vector2<i32>,
    pub remove_offset: Vector2<i32>, // Is only used for final shift
    pub frameparts: Vec<FramePart>,
}

impl Frame {
    // This function generate new rame
    pub fn merge_frame_part(frames: &mut Vec<Frame>) -> Frame {
        for frame in frames.iter_mut() {
            // Finding the half size of the image
            let half_size = (frame.size - Vector2::new(1, 1)) / 2;
            frame.upleft = frame.offset - half_size;
        }
        // If there is no frame return an image with 1 pixel.
        if frames.is_empty() {
            let frame_part = FramePart {
                animation_index: -1,
                sprite_number: -1,
                size: Vector2::new(1, 1),
                upleft: Vector2::zero(),
                offset: Vector2::zero(),
                mirror: false,
                angle: 0.0,
                color: Color {
                    red: 0.0,
                    blue: 0.0,
                    green: 0.0,
                    alpha: 0.0,
                },
            };
            let frame = Frame {
                size: Vector2::new(1, 1),
                upleft: Vector2::zero(),
                offset: Vector2::zero(),
                remove_offset: Vector2::zero(),
                frameparts: vec![frame_part],
            };
            return frame;
        }
        // Find the upmost and leftmost coordinates
        let upleft_x = frames.iter().min_by_key(|frame| frame.upleft.x).unwrap().upleft.x;
        let upleft_y = frames.iter().min_by_key(|frame| frame.upleft.y).unwrap().upleft.y;

        // Find the downmost and rightmost coordinates
        let frame_x = frames.iter().max_by_key(|frame| frame.upleft.x + frame.size.x as i32).unwrap();
        let frame_y = frames.iter().max_by_key(|frame| frame.upleft.y + frame.size.y as i32).unwrap();

        // Calculate the new rectangle that is formed
        let mut new_width = frame_x.upleft.x + frame_x.size.x as i32;
        let mut new_height = frame_y.upleft.y + frame_y.size.y as i32;
        new_width -= upleft_x;
        new_height -= upleft_y;

        let mut new_frameparts = Vec::<FramePart>::new();
        for index in 0..frames.len() {
            new_frameparts.append(&mut frames[index].frameparts);
        }

        /* As the origin is (0,0), you get the upleft point of the retangle and
         * shift to the center, the offset is the difference between the origin and
         * this point */
        Frame {
            size: Vector2::new(new_width, new_height),
            upleft: Vector2::zero(),
            offset: Vector2::new(upleft_x + (new_width - 1) / 2, upleft_y + (new_height - 1) / 2),
            remove_offset: Vector2::zero(),
            frameparts: new_frameparts,
        }
    }
}

#[derive(Clone, PrototypeElement)]
pub struct Animation {
    #[hidden_element]
    pub frames: Vec<Frame>,
}

#[derive(Clone, PrototypeElement)]
pub struct AnimationPair {
    pub sprites: Arc<Sprite>,
    pub actions: Arc<Actions>,
}

#[derive(Clone, PrototypeElement)]
pub struct AnimationData {
    pub animation_pair: Vec<AnimationPair>,
    pub animations: Vec<Animation>,
    pub delays: Vec<f32>,
    #[hidden_element]
    pub entity_type: EntityType,
}

// This function generates the convertion to the big rectangle
fn convert_coordinate(coordinate: Vector2<i32>, size: Vector2<i32>) -> Vector2<f32> {
    let x = (coordinate.x as f32 / size.x as f32 - 0.5) * 2.0;
    let y = 2.0 - (coordinate.y as f32 / size.y as f32) * 2.0;
    return Vector2::<f32>::new(x, y);
}

impl AnimationData {
    pub fn render(
        &self,
        animation_state: &AnimationState,
        camera_direction: usize,
        head_direction: usize,
    ) -> Vec<(Arc<Texture>, Vec<Vector2<f32>>, Vector2<f32>, Vector2<i32>, f32, Color, bool)> {
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
        // TODO: work out how to avoid losing digits when casting timg to an f32. When
        // fixed remove set_start_time in MouseCursor.

        let mut frame = &animation.frames[0];
        let size = frame.size;
        let time = frame_time as usize % animation.frames.len();

        // Remove Doridori animation from Player
        if self.entity_type == EntityType::Player && animation_state.action == 0 {
            frame = &animation.frames[0];
        } else {
            frame = &animation.frames[time];
        }

        let mut datas = Vec::new();
        for framepart in frame.frameparts.iter() {
            let texture;
            let position;
            let animation_index = framepart.animation_index as usize;
            let sprite_number = framepart.sprite_number as usize;
            texture = &self.animation_pair[animation_index].sprites.textures[sprite_number];
            position = Vector2::new(
                animation.frames[0].offset.x as f32,
                animation.frames[0].offset.y as f32 + ((animation.frames[time].size.y - 1) / 2) as f32,
            ) / 10.0;

            // Generate vertex
            let new_vector = framepart.size - Vector2::new(1, 1);
            let old_origin = frame.offset - (frame.size - frame.remove_offset - Vector2::new(1, 1)) / 2;
            let new_origin = framepart.offset - (framepart.size - Vector2::new(1, 1)) / 2;
            let top_left = new_origin - old_origin;
            let bottom_left = top_left + new_vector.y * Vector2::<i32>::unit_y();
            let top_right = top_left + new_vector.x * Vector2::<i32>::unit_x();
            let bottom_right = top_left + new_vector;

            let texture_top_left = convert_coordinate(top_left, frame.size);
            let texture_bottom_left = convert_coordinate(bottom_left, frame.size);
            let texture_top_right = convert_coordinate(top_right, frame.size);
            let texture_bottom_right = convert_coordinate(bottom_right, frame.size);

            let mut texture_coordinates = Vec::new();
            texture_coordinates.push(texture_top_left);
            texture_coordinates.push(texture_bottom_left);
            texture_coordinates.push(texture_top_right);
            texture_coordinates.push(texture_bottom_right);

            datas.push((
                texture.clone(),
                texture_coordinates,
                position,
                size,
                framepart.angle,
                framepart.color,
                framepart.mirror,
            ));
        }
        datas
    }
}
