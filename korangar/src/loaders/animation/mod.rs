use std::cmp::{max, min};
use std::sync::Arc;

use cgmath::{Matrix4, Vector2};
use derive_new::new;
use hashbrown::HashMap;
use num::Zero;

use super::error::LoadError;
use crate::loaders::{ActionLoader, SpriteLoader};
use crate::world::{Animation, AnimationData, AnimationFrame, AnimationFramePart, AnimationPair};
use crate::{Color, EntityType};

// TODO: NHA Create and use an easier to use cache.
#[derive(new)]
pub struct AnimationLoader {
    #[new(default)]
    cache: HashMap<Vec<String>, Arc<AnimationData>>,
}

impl AnimationLoader {
    pub fn load(
        &mut self,
        sprite_loader: &mut SpriteLoader,
        action_loader: &mut ActionLoader,
        entity_type: EntityType,
        entity_part_files: &[String],
    ) -> Result<Arc<AnimationData>, LoadError> {
        // Create animation pair with sprite and action
        let animation_pairs: Vec<AnimationPair> = entity_part_files
            .iter()
            .map(|file_path| AnimationPair {
                sprites: sprite_loader.get(&format!("{file_path}.spr")).unwrap(),
                actions: action_loader.get(&format!("{file_path}.act")).unwrap(),
            })
            .collect();

        // The sprite is stored as rgba in animation_pair.sprites.rgba_images or
        // animation_pair.sprites.palette_images
        // The sprite is stored as texture in animation_pair.sprites.textures

        // For each animation, we collect all the frame part need to generate the frame
        let mut animations_list: Vec<Vec<Vec<AnimationFrame>>> = Vec::new();

        // Each animation pair has the sprites and actions, we iterate over the
        // animation pairs.
        // Each entity has several actions and the actions is composed of several
        // motion, in each motion contains several pictures that we try to
        // merge.
        for (animation_index, animation_pair) in animation_pairs.iter().enumerate() {
            let mut animation_frames: Vec<Vec<AnimationFrame>> = Vec::new();
            for (action_index, action) in animation_pair.actions.actions.iter().enumerate() {
                let mut action_frames: Vec<AnimationFrame> = Vec::new();
                for (motion_index, motion) in action.motions.iter().enumerate() {
                    let mut motion_frames: Vec<AnimationFrame> = Vec::new();
                    if motion.sprite_clip_count == 0 {
                        continue;
                    }
                    for sprite_clip in motion.sprite_clips.iter() {
                        if sprite_clip.sprite_number == -1 {
                            continue;
                        }
                        // Find the correct sprite number
                        let mut sprite_number = sprite_clip.sprite_number as usize;
                        // The type of sprite 0 for pallete, 1 for bgra
                        let sprite_type = match sprite_clip.sprite_type {
                            Some(value) => value as usize,
                            None => 0,
                        };
                        if sprite_type == 1 {
                            sprite_number += animation_pair.sprites.palette_size;
                        }

                        // Find the size of image
                        let texture_size = animation_pair.sprites.textures[sprite_number].get_size();
                        let mut height = texture_size.height;
                        let mut width = texture_size.width;

                        // Get the value to apply color filter in the image
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

                        // Scales the image. Try to match the first type of zoom, if it doesn't match
                        // find the second method.
                        let zoom = match sprite_clip.zoom {
                            Some(value) => (value, value).into(),
                            None => sprite_clip.zoom2.unwrap_or_else(|| (1.0, 1.0).into()),
                        };
                        if zoom != (1.0, 1.0).into() {
                            width = (width as f32 * zoom.x).ceil() as u32;
                            height = (height as f32 * zoom.y).ceil() as u32;
                        }

                        // Get the image rotation angle
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
                        // Attach point have a different offset calculation.
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

                        let frame_part = AnimationFramePart {
                            animation_index,
                            sprite_number,
                            size,
                            offset,
                            mirror,
                            angle,
                            color,
                            ..Default::default()
                        };
                        let frame = AnimationFrame {
                            size,
                            top_left: Vector2::zero(),
                            offset,
                            remove_offset: Vector2::zero(),
                            frame_parts: vec![frame_part],
                        };
                        motion_frames.push(frame);
                    }
                    if motion_frames.len() == 1 {
                        action_frames.push(motion_frames[0].clone());
                    } else {
                        let frame = merge_frame(&mut motion_frames);
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

        // For each action generate all the motions.
        // For each motion get the animation pair for merging.
        // Merge the animation pair and get the frame for each action.
        for action_index in 0..action_size {
            let motion_size = animation_pairs[0].actions.actions[action_index].motions.len();
            let mut frames: Vec<AnimationFrame> = Vec::new();
            for motion_index in 0..motion_size {
                let mut generate: Vec<AnimationFrame> = Vec::new();

                for pair in &animations_list[0..animation_pair_size] {
                    if pair.len() <= action_index {
                        continue;
                    }
                    if pair[action_index].len() <= motion_index {
                        continue;
                    }
                    generate.push(pair[action_index][motion_index].clone());
                }
                let frame = merge_frame(&mut generate);

                frames.push(frame);
            }

            // The problem is that each frame from action is not in the same size
            // and without the same size the proportion is different.
            // To solve this primarily, we created images of the same size and same offset
            // This code resize the frame in the same size and same offset.
            // Initially we find the max width and height and max and min offsets.
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
                for frame_part in frame.frame_parts.iter_mut() {
                    frame_part.offset.x += max_offset_x - min_offset_x + 2;
                    frame_part.offset.y += max_offset_y - min_offset_y + 2;
                    // Precompute the vertex for rendering later
                    let new_vector = frame_part.size;
                    let old_origin = frame.offset - (frame.size - frame.remove_offset - Vector2::new(1, 1)) / 2;
                    let new_origin = frame_part.offset - (frame_part.size - Vector2::new(1, 1)) / 2;
                    let top_left = new_origin - old_origin;
                    let bottom_left = top_left + new_vector.y * Vector2::<i32>::unit_y();
                    let bottom_right = top_left + new_vector;

                    let texture_top_left = convert_coordinate(top_left, frame.size);
                    let texture_bottom_left = convert_coordinate(bottom_left, frame.size);
                    let texture_bottom_right = convert_coordinate(bottom_right, frame.size);

                    // Scale the vertex (2.-1), (0, -1), (2, 1), (0, 1) to fit texture coordinates
                    // as above.
                    let scale_matrix = Matrix4::from_nonuniform_scale(
                        (texture_bottom_right.x - texture_bottom_left.x) / 2.0,
                        (texture_top_left.y - texture_bottom_left.y) / 2.0,
                        1.0,
                    );

                    // Center from the vertices of truth table
                    let new_center = Vector2::new(0.0, (texture_top_left.y - texture_bottom_left.y) / 2.0);
                    // Center from the texture center
                    let texture_center = (texture_top_left + texture_bottom_right) / 2.0;
                    // Find the amount of translation needed
                    let translation_matrix = Matrix4::from_translation(texture_center.extend(1.0) - new_center.extend(1.0));

                    frame_part.affine_matrix = translation_matrix * scale_matrix;
                }
            });
            animations.push(Animation { frames });
        }

        let animation_data = Arc::new(AnimationData {
            delays: animation_pairs[0].actions.delays.clone(),
            animation_pair: animation_pairs,
            animations,
            entity_type,
        });

        self.cache.insert(entity_part_files.to_vec(), animation_data.clone());

        Ok(animation_data)
    }

    pub fn get(
        &mut self,
        sprite_loader: &mut SpriteLoader,
        action_loader: &mut ActionLoader,
        entity_type: EntityType,
        entity_part_files: &[String],
    ) -> Result<Arc<AnimationData>, LoadError> {
        match self.cache.get(entity_part_files) {
            Some(animation_data) => Ok(animation_data.clone()),
            None => self.load(sprite_loader, action_loader, entity_type, entity_part_files),
        }
    }
}

// This function convert to the "normalized" coordinates of a frame part inside
// the frame bounding box rectangle with vertex [-1, 0], [-1, 2], [1, 0], [1, 2]
fn convert_coordinate(coordinate: Vector2<i32>, size: Vector2<i32>) -> Vector2<f32> {
    const EPSILON: f32 = 0.0001;
    let x = (coordinate.x as f32 / size.x as f32 - 0.5) * 2.0 + EPSILON;
    let y = 2.0 - (coordinate.y as f32 / size.y as f32) * 2.0 + EPSILON;
    Vector2::<f32>::new(x, y)
}

/// This function generates a new frame by merging a list of frames.
fn merge_frame(frames: &mut [AnimationFrame]) -> AnimationFrame {
    for frame in frames.iter_mut() {
        // Finding the half size of the image
        // For even side and the side have length 4, the center coordinate is 1.
        // For odd side and the side have length 3, the center coordinate is 1.
        let half_size = (frame.size - Vector2::new(1, 1)) / 2;
        frame.top_left = frame.offset - half_size;
    }

    // If there is no frame return an image with 1 pixel.
    if frames.is_empty() {
        let frame_part = AnimationFramePart {
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
        let frame = AnimationFrame {
            size: Vector2::new(1, 1),
            top_left: Vector2::zero(),
            offset: Vector2::zero(),
            remove_offset: Vector2::zero(),
            frame_parts: vec![frame_part],
        };
        return frame;
    }

    // Find the upmost and leftmost coordinates
    let top_left_x = frames.iter().min_by_key(|frame| frame.top_left.x).unwrap().top_left.x;
    let top_left_y = frames.iter().min_by_key(|frame| frame.top_left.y).unwrap().top_left.y;

    // Find the bottommost and rightmost coordinates
    let frame_x = frames.iter().max_by_key(|frame| frame.top_left.x + frame.size.x).unwrap();
    let frame_y = frames.iter().max_by_key(|frame| frame.top_left.y + frame.size.y).unwrap();

    // Calculate the new rectangle that is formed
    let new_width = (frame_x.top_left.x + frame_x.size.x) - top_left_x;
    let new_height = (frame_y.top_left.y + frame_y.size.y) - top_left_y;

    let mut new_frame_parts = Vec::with_capacity(frames.iter().map(|frame| frame.frame_parts.len()).sum());
    for frame in frames.iter_mut() {
        new_frame_parts.append(&mut frame.frame_parts);
    }

    // The origin is (0,0).
    //
    // The top left point of the rectangle is calculated by
    // origin + offset - half_size
    //
    // The center point of the rectangle is calculated by
    // top_left_point +  half_size
    //
    // The new offset is calculated by
    // center_point - origin.
    AnimationFrame {
        size: Vector2::new(new_width, new_height),
        top_left: Vector2::zero(),
        offset: Vector2::new(top_left_x + (new_width - 1) / 2, top_left_y + (new_height - 1) / 2),
        remove_offset: Vector2::zero(),
        frame_parts: new_frame_parts,
    }
}
