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
    cache: HashMap<usize, AnimationData>,
}

impl AnimationLoader {
    pub fn load(
        &mut self,
        sprite_loader: &mut SpriteLoader,
        action_loader: &mut ActionLoader,
        entity_filename: Vec<String>,
        entity_hash: Vec<usize>,
        entity_type: EntityType,
    ) -> Result<AnimationData, LoadError> {
        // Create animation pair with sprite and action
        let animation_pairs: Vec<AnimationPair> = entity_filename
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

        // Each animation pair has the sprites and actions, we iterate over the
        // animation pairs
        // Each entity has several actions and the actions is composed of several
        // motion, in each motion contains several pictures that we try to
        // merge.
        for (animation_index, animation_pair) in animation_pairs.iter().enumerate() {
            let mut animation_frames: Vec<Vec<Frame>> = Vec::new();
            for (action_index, action) in animation_pair.actions.actions.iter().enumerate() {
                let mut action_frames: Vec<Frame> = Vec::new();
                for (motion_index, motion) in action.motions.iter().enumerate() {
                    let mut motion_frames: Vec<Frame> = Vec::new();
                    if motion.sprite_clip_count == 0 {
                        continue;
                    }
                    for sprite_clip in motion.sprite_clips.iter() {
                        if sprite_clip.sprite_number == -1 {
                            continue;
                        }
                        let sprite_number = sprite_clip.sprite_number as usize;
                        let texture_size = animation_pair.sprites.textures[sprite_number].get_size();
                        let mut height = texture_size.height;
                        let mut width = texture_size.width;
                        // Apply color filter in the image
                        let color = match sprite_clip.color {
                            Some(color) => {
                                let alpha = (((color >> 24) & 0xFF) as u8) as f32 / 255.0;
                                let blue = (((color >> 16) & 0xFF) as u8) as f32 / 255.0;
                                let green = (((color >> 8) & 0xFF) as u8) as f32 / 255.0;
                                let red = (((color) & 0xFF) as u8) as f32 / 255.0;

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
                        let zoom = match sprite_clip.zoom {
                            Some(value) => (value, value).into(),
                            None => sprite_clip.zoom2.unwrap_or_else(|| (1.0, 1.0).into()),
                        };
                        if zoom != (1.0, 1.0).into() {
                            width = (width as f32 * zoom.x).floor() as u32;
                            height = (height as f32 * zoom.y).floor() as u32;
                        }
                        // Get the image rotation
                        let angle = match sprite_clip.angle {
                            Some(value) => value as f32 / 360.0 * 2.0 * std::f32::consts::PI,
                            None => 0.0,
                        };
                        // Get the offset and if the image is mirrored
                        let mut offset = sprite_clip.position.map(|component| component);
                        let mirror = sprite_clip.mirror_on != 0;

                        // This is hardcoded for head in player for attach_point
                        // animation_index == 0 is head
                        // animation_index == 1 is body
                        let has_attach_point = match motion.attach_point_count {
                            Some(value) => value == 1,
                            None => false,
                        };
                        if entity_type == EntityType::Player && has_attach_point && animation_index == 1 {
                            let parent_animation_pair = &animation_pairs[0];
                            let parent_action = &parent_animation_pair.actions.actions[action_index];
                            let parent_motion = &parent_action.motions[motion_index];
                            let parent_attach_point = parent_motion.attach_points[0].position;
                            let attach_point = motion.attach_points[0].position;
                            let new_offset = -attach_point + parent_attach_point;
                            offset += new_offset;
                        }
                        let size = Vector2::new(width as i32, height as i32);

                        let frame_part = FramePart {
                            animation_index,
                            sprite_number,
                            size,
                            offset,
                            mirror,
                            angle,
                            color,
                            ..Default::default()
                        };
                        let frame = Frame {
                            size,
                            top_left: Vector2::zero(),
                            offset,
                            remove_offset: Vector2::zero(),
                            frameparts: vec![frame_part],
                        };
                        motion_frames.push(frame);
                    }
                    if motion_frames.len() == 1 {
                        action_frames.push(motion_frames[0].clone());
                    } else {
                        let frame = Frame::merge_frame(&mut motion_frames);
                        action_frames.push(frame);
                    }
                }
                animation_frames.push(action_frames);
            }
            animations_list.push(animation_frames);
        }
        let action_size = animation_pairs[0].actions.actions.len();
        let animation_pair_size = animation_pairs.len();

        let mut animations: Vec<Animation> = Vec::new();

        // For each action generate all the motions
        // For each motion get the animation pair for merging.
        // Merge the animation pair and get the frame for each action
        for action_index in 0..action_size {
            let motion_size = animation_pairs[0].actions.actions[action_index].motions.len();
            let mut frames: Vec<Frame> = Vec::new();
            for motion_index in 0..motion_size {
                let mut generate: Vec<Frame> = Vec::new();

                for pair in &animations_list[0..animation_pair_size] {
                    if pair.len() <= action_index {
                        continue;
                    }
                    if pair[action_index].len() <= motion_index {
                        continue;
                    }
                    generate.push(pair[action_index][motion_index].clone());
                }
                let frame = Frame::merge_frame(&mut generate);

                frames.push(frame);
            }

            // The problem is that each frame from action is not in the same size
            // and without the same size the proportion is different.
            // To solve this primarly, we created images of the same size and same offset
            // This code resize the frame  in the same size and same offset
            // Initially we find the max width and height and max and min offsets
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
            // Add the different offset in the frame size.
            // The constant 2 is to avoid miscalculation by 1.
            max_width += max_offset_x - min_offset_x + 2;
            max_height += max_offset_y - min_offset_y + 2;

            // Shift every frame by max_offset_x - min_offset_x + 2
            // As the bottom part and left part will not be in the offset.
            frames.iter_mut().for_each(|frame| {
                frame.offset.x = min_offset_x;
                frame.offset.y = min_offset_y;
                frame.offset.x += max_offset_x - min_offset_x + 2;
                frame.offset.y += max_offset_y - min_offset_y + 2;
                frame.size = Vector2::new(max_width, max_height);
                frame.remove_offset.x = max_offset_x - min_offset_x + 2;
                frame.remove_offset.y = max_offset_y - min_offset_y + 2;
                for framepart in frame.frameparts.iter_mut() {
                    framepart.offset.x += max_offset_x - min_offset_x + 2;
                    framepart.offset.y += max_offset_y - min_offset_y + 2;
                    // Precompute the vertex for rendering later
                    let new_vector = framepart.size - Vector2::new(1, 1);
                    let old_origin = frame.offset - (frame.size - frame.remove_offset - Vector2::new(1, 1)) / 2;
                    let new_origin = framepart.offset - (framepart.size - Vector2::new(1, 1)) / 2;
                    let top_left = new_origin - old_origin;
                    let bottom_left = top_left + new_vector.y * Vector2::<i32>::unit_y();
                    let top_right = top_left + new_vector.x * Vector2::<i32>::unit_x();
                    let bottom_right = top_left + new_vector;

                    framepart.texture_top_left = convert_coordinate(top_left, frame.size);
                    framepart.texture_bottom_left = convert_coordinate(bottom_left, frame.size);
                    framepart.texture_top_right = convert_coordinate(top_right, frame.size);
                    framepart.texture_bottom_right = convert_coordinate(bottom_right, frame.size);
                }
            });
            animations.push(Animation { frames });
        }
        let animation_data = AnimationData {
            delays: animation_pairs[0].actions.delays.clone(),
            animation_pair: animation_pairs,
            animations,
            entity_type,
        };
        let hash = match entity_type {
            EntityType::Player => 30000 + entity_hash[0] * 2 + entity_hash[1],
            EntityType::Monster => entity_hash[0],
            EntityType::Npc => entity_hash[0],
            _ => entity_hash[0],
        };

        self.cache.insert(hash, animation_data.clone());
        Ok(animation_data)
    }

    pub fn get(
        &mut self,
        sprite_loader: &mut SpriteLoader,
        action_loader: &mut ActionLoader,
        entity_filename: Vec<String>,
        entity_hash: Vec<usize>,
        entity_type: EntityType,
    ) -> Result<AnimationData, LoadError> {
        let hash = match entity_type {
            EntityType::Player => 30000 + entity_hash[0] * 2 + entity_hash[1],
            EntityType::Monster => entity_hash[0],
            EntityType::Npc => entity_hash[0],
            _ => entity_hash[0],
        };

        match self.cache.get(&hash) {
            Some(animation_data) => Ok(animation_data.clone()),
            None => self.load(sprite_loader, action_loader, entity_filename, entity_hash, entity_type),
        }
    }
}

#[derive(Clone)]
pub struct FramePart {
    pub animation_index: usize,
    pub sprite_number: usize,
    pub offset: Vector2<i32>,
    pub size: Vector2<i32>,
    pub mirror: bool,
    pub angle: f32,
    pub color: Color,
    pub texture_top_left: Vector2<f32>,
    pub texture_bottom_left: Vector2<f32>,
    pub texture_top_right: Vector2<f32>,
    pub texture_bottom_right: Vector2<f32>,
}
// Doesn't know how to properly default cgmath::Vector2<T>
impl Default for FramePart {
    fn default() -> FramePart {
        FramePart {
            animation_index: usize::MAX,
            sprite_number: usize::MAX,
            offset: Vector2::<i32>::zero(),
            size: Vector2::<i32>::zero(),
            mirror: Default::default(),
            angle: Default::default(),
            color: Default::default(),
            texture_top_left: Vector2::<f32>::zero(),
            texture_bottom_left: Vector2::<f32>::zero(),
            texture_top_right: Vector2::<f32>::zero(),
            texture_bottom_right: Vector2::<f32>::zero(),
        }
    }
}

#[derive(Clone)]
pub struct Frame {
    pub offset: Vector2<i32>,
    pub top_left: Vector2<i32>,
    pub size: Vector2<i32>,
    pub remove_offset: Vector2<i32>, // Is only used for final shift
    pub frameparts: Vec<FramePart>,
}

impl Frame {
    /// This function generate a new frame by merging a list of frames.
    pub fn merge_frame(frames: &mut [Frame]) -> Frame {
        for frame in frames.iter_mut() {
            // Finding the half size of the image
            let half_size = (frame.size - Vector2::new(1, 1)) / 2;
            frame.top_left = frame.offset - half_size;
        }
        // If there is no frame return an image with 1 pixel.
        if frames.is_empty() {
            let frame_part = FramePart {
                animation_index: usize::MAX,
                sprite_number: usize::MAX,
                size: Vector2::new(1, 1),
                offset: Vector2::zero(),
                mirror: false,
                angle: 0.0,
                color: Color {
                    red: 0.0,
                    blue: 0.0,
                    green: 0.0,
                    alpha: 0.0,
                },
                ..Default::default()
            };
            let frame = Frame {
                size: Vector2::new(1, 1),
                top_left: Vector2::zero(),
                offset: Vector2::zero(),
                remove_offset: Vector2::zero(),
                frameparts: vec![frame_part],
            };
            return frame;
        }
        // Find the upmost and leftmost coordinates
        let top_left_x = frames.iter().min_by_key(|frame| frame.top_left.x).unwrap().top_left.x;
        let top_left_y = frames.iter().min_by_key(|frame| frame.top_left.y).unwrap().top_left.y;

        // Find the downmost and rightmost coordinates
        let frame_x = frames.iter().max_by_key(|frame| frame.top_left.x + frame.size.x).unwrap();
        let frame_y = frames.iter().max_by_key(|frame| frame.top_left.y + frame.size.y).unwrap();

        // Calculate the new rectangle that is formed
        let mut new_width = frame_x.top_left.x + frame_x.size.x;
        let mut new_height = frame_y.top_left.y + frame_y.size.y;
        new_width -= top_left_x;
        new_height -= top_left_y;

        let mut new_frameparts = Vec::<FramePart>::new();
        for frame in frames.iter_mut() {
            new_frameparts.append(&mut frame.frameparts);
        }

        // The origin is (0,0).
        // The top left point of the rectangle is calculated by
        // origin + offset - half_size.
        // The center point of the rectangle is calculated by
        // top_left_point +  half_size.
        // The new offset is calculated by
        // center_point - origin.
        Frame {
            size: Vector2::new(new_width, new_height),
            top_left: Vector2::zero(),
            offset: Vector2::new(top_left_x + (new_width - 1) / 2, top_left_y + (new_height - 1) / 2),
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

// This function convert to the "normalized" coordinates of a frame part inside
// the frame bounding box rectangle with vertex [-1, 0], [-1, 2], [1, 0], [1, 2]
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
        let time = frame_time as usize % animation.frames.len();
        let mut frame = &animation.frames[time];

        // Remove Doridori animation from Player
        if self.entity_type == EntityType::Player && animation_state.action == 0 {
            frame = &animation.frames[0];
        }

        let mut datas = Vec::new();
        for framepart in frame.frameparts.iter() {
            let animation_index = framepart.animation_index as usize;
            let sprite_number = framepart.sprite_number as usize;
            let texture = &self.animation_pair[animation_index].sprites.textures[sprite_number];

            // The constant 10.0 is a magic scale factor of image.
            // The position of vertex is calculated from the center of image, so we need to
            // add half of the height.
            let position = Vector2::new(
                animation.frames[0].offset.x as f32,
                animation.frames[0].offset.y as f32 + ((animation.frames[time].size.y - 1) / 2) as f32,
            ) / 10.0;

            let texture_coordinates = vec![
                framepart.texture_top_left,
                framepart.texture_bottom_left,
                framepart.texture_top_right,
                framepart.texture_bottom_right,
            ];

            datas.push((
                texture.clone(),
                texture_coordinates,
                position,
                frame.size,
                framepart.angle,
                framepart.color,
                framepart.mirror,
            ));
        }
        datas
    }
}
