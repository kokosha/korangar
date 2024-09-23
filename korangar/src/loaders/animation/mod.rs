use std::collections::HashMap;
use std::sync::Arc;

use derive_new::new;

use ragnarok_formats::sprite::RgbaImageData;
use korangar_interface::elements::PrototypeElement;
use crate::loaders::{Sprite, Actions, ActionLoader, SpriteLoader, AnimationState};
use image::{save_buffer,RgbaImage}; 

use wgpu::{Device, Extent3d, Queue, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages};
use crate::graphics::Texture;

#[derive(new)]
pub struct AnimationLoader {
    device: Arc<Device>,
    queue: Arc<Queue>,
    #[new(default)]
    cache: HashMap<String, Arc<AnimationData>>,
}

impl AnimationLoader{
    pub fn get_animation_data(&mut self,
        sprite_loader: &mut SpriteLoader,
        action_loader: &mut ActionLoader,
        entity_filename: Vec<String>,
    ) -> AnimationData{
    
        let vec: Vec<AnimationPair> = entity_filename
        .iter()
        .map(|file_path| {
        (
            AnimationPair{
                sprites: sprite_loader.get(&format!("{file_path}.spr")).unwrap(),
                actions: action_loader.get(&format!("{file_path}.act")).unwrap()}
        )
        }).collect();
    
        // 1 - Generate all the sprites in rgba
        // Stored in animation_pair.sprites.rgba_images from vec.
    
        // 2 - Get the actions for merging the sprites in one
        // Each action have a vector of framepart
        let mut vec3_frame_part : Vec<Vec<Vec<FramePart>>> = Vec::new();
        let mut animation_index: usize = 0;
        for animation_pair in vec.iter() {
            let mut vec2_frame_part : Vec<Vec<FramePart>> = Vec::new();
            for action in animation_pair.actions.actions.iter() {
                let mut vec_frame_part : Vec<FramePart> = Vec::new();
                for motion in action.motions.iter() {
                    let frame_part: FramePart;
                    let pos = motion.sprite_clip_count as usize -1;
                    let sprite_number = motion.sprite_clips[pos].sprite_number as usize;
                    let rgba_image = vec[animation_index].sprites.rgba_images[sprite_number].clone();
                    let offset = motion.sprite_clips[pos].position.map(|component| component);
                    let mirror = motion.sprite_clips[pos].mirror_on != 0;
    
                    let mut has_attach_point = motion.attach_point_count.unwrap() == 1;
                    let mut attach_point_x = 0;
                    let mut attach_point_y = 0;
    
                    if has_attach_point {
                        attach_point_x = motion.attach_points[0].position.x;
                        attach_point_y = motion.attach_points[0].position.y;
                    }
                    let mut attach_point_parent_x = 0;
                    let mut attach_point_parent_y = 0;
    
                    let sprite_type = match animation_index {
                        0 => SpriteType::Body,
                        1 => SpriteType::Head,
                        _ => SpriteType::Other, 
                    };
                    frame_part = FramePart{
                                sprite_type: sprite_type,
                                rgba_data:rgba_image.clone(),
                                offset_x: offset.x,
                                offset_y: offset.y,
                                attach_point_x,
                                attach_point_y,  
                                has_attach_point,  
                                mirror,
                                attach_point_parent_x,
                                attach_point_parent_y,
                    };
                   
                    vec_frame_part.push(frame_part);
                }
                vec2_frame_part.push(vec_frame_part);
            }
            vec3_frame_part.push(vec2_frame_part);
            animation_index += 1;
        }
        let action_size = vec[0].actions.actions.len();
        let animation_pair_size = vec.len();
        
        let mut animations : Vec<Animation> = Vec::new();

        for action_index in 0..action_size{
            let motion_size =  vec[0].actions.actions[action_index].motions.len();
            let mut rgba_images: Vec<RgbaImageData> = Vec::new();
            for motion_index in 0..motion_size {
                let mut generate: Vec<FramePart> = Vec::new();
                
                if vec3_frame_part.len() == 2 {  //TODO: THIS IS HARD CODED HEAD, NEED TO UPDATE THE CODE
                    vec3_frame_part[1][action_index][motion_index].attach_point_parent_x = vec3_frame_part[0][action_index][motion_index].attach_point_x;
                    vec3_frame_part[1][action_index][motion_index].attach_point_parent_y = vec3_frame_part[0][action_index][motion_index].attach_point_y;       
                }
                for animation_pair_index in 0..animation_pair_size{
                    generate.push(vec3_frame_part[animation_pair_index][action_index][motion_index].clone());
                }
                let rgba: RgbaImageData = Frame::merge_frame_part(generate);
                rgba_images.push(rgba);
            }
            // 3 - Create the textures using the animation loader functions.

            let label = format!("{}_{}", entity_filename[0], action_index);
            let textures: Vec<Arc<Texture>> = rgba_images.into_iter()
            .map(|image_data| {
                let texture = Texture::new_with_data(
                    &self.device,
                    &self.queue,
                    &TextureDescriptor {
                        label: Some(&label),
                        size: Extent3d {
                            width: image_data.width as u32,
                            height: image_data.height as u32,
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: TextureDimension::D2,
                        format: TextureFormat::Rgba8UnormSrgb,
                        usage: TextureUsages::COPY_DST | TextureUsages::TEXTURE_BINDING,
                        view_formats: &[],
                    },
                    &image_data.data,
                );
                Arc::new(texture)
            })
            .collect();
            animations.push(Animation{
                textures,
            });
        }
        AnimationData{
            animations,
            animation_pair: vec
        }
        
    }

   
}

#[derive(Clone)]
pub enum SpriteType {
    Head,
    Body,
    Other,
}

#[derive(Clone)]
pub struct FramePart{
    pub sprite_type: SpriteType,
    pub rgba_data: RgbaImageData,
    pub offset_x: i32,
    pub offset_y: i32,
    pub has_attach_point: bool,
    pub attach_point_x: i32,
    pub attach_point_y: i32,
    pub attach_point_parent_x: i32,
    pub attach_point_parent_y: i32,  

    pub mirror: bool,
}

#[derive(PrototypeElement)]
pub struct AnimationPair {
    pub sprites: Arc<Sprite>,
    pub actions: Arc<Actions>,
}
pub struct Frame{
    pub texture: Arc<Texture>,
}
#[derive(PrototypeElement)]
pub struct Animation{
    #[hidden_element]
    pub textures: Vec<Arc<Texture>>, // The vector of frames generated from animation pair
}

#[derive(PrototypeElement)]
pub struct AnimationData{
    pub animations: Vec<Animation>,
    pub animation_pair: Vec<AnimationPair>, 
    // The string will be type of entity
    // {entity_type}_{body_id}_{head_id}_{action_id}_{motion_id}
}


impl Frame{
    /// The generate image will be overwrite in the order of the index of the vector
    //fn merge_rgba_image_data(vec_sprite_data: Vec<SpriteData>, vec_action_data: Vec<ActionData>) -> RgbaImageData{
    pub fn merge_frame_part(mut vec_frame_part: Vec<FramePart>) -> RgbaImageData{

        // Adjusting the values
        for it in vec_frame_part.iter_mut(){
            let mirror_offset = match it.mirror {
                true => -1,
                false => 1
            };
            let attach_point_offset_x = match &it.has_attach_point {
                    true => match &it.sprite_type {
                        SpriteType::Head => -it.attach_point_x + it.attach_point_parent_x,
                        _ => 0,
                    }
                    false => 0,
                };
            let attach_point_offset_y = match &it.has_attach_point {
                true => match &it.sprite_type {
                    SpriteType::Head => -it.attach_point_y + it.attach_point_parent_y,
                    _ => 0,
                }
                false => 0,
            };
            it.offset_x = it.offset_x -(it.rgba_data.width as i32 + mirror_offset)/2  + attach_point_offset_x;
            it.offset_y = it.offset_y -(it.rgba_data.height as i32 + mirror_offset)/2 + attach_point_offset_y;
        }
        /// Create a new RgbaImageData from the two RgbaImageData
        let rgba: RgbaImageData;
        /// Get the minimal offset to find the new pixel (0, 0)
        let mut offset_x = vec_frame_part.iter().min_by_key(|it| it.offset_x).unwrap().offset_x;
        let mut offset_y = vec_frame_part.iter().min_by_key(|it| it.offset_y).unwrap().offset_y;

        /// The new size of the rgba
        let mut it_1 = vec_frame_part.iter().max_by_key(|it| it.offset_x + it.rgba_data.width as i32).unwrap();
        let mut it_2 = vec_frame_part.iter().max_by_key(|it| it.offset_y + it.rgba_data.height as i32).unwrap();

        let mut new_width = it_1.offset_x + it_1.rgba_data.width as i32;
        let mut new_height = it_2.offset_y + it_2.rgba_data.height as i32;
        new_width -= offset_x;
        new_height -= offset_y;

        /// Create a ImageBuffer
        let mut rgba: RgbaImage = RgbaImage::new(new_width as u32, new_height as u32) ;

        /// Convert to the format
        let mut vec_rgba: Vec<RgbaImage> = Vec::new();
        for index in 0..vec_frame_part.len() {
            let temp: RgbaImage = RgbaImage::from_raw(vec_frame_part[index].rgba_data.width.into(), vec_frame_part[index].rgba_data.height.into(), vec_frame_part[index].rgba_data.data.clone()).unwrap();
            vec_rgba.push(temp);
        }

        /// The order of for is important for cache
        /// Insert the images in the new image in the order of the index of the vector
        for index in 0..vec_rgba.len() {
            let height = vec_rgba[index].height();
            let width = vec_rgba[index].width();
            for y in 0..height {
                let new_y =  (y as i32) + vec_frame_part[index].offset_y - offset_y;
                for x in 0..width {
                    let new_x =  x as i32 + vec_frame_part[index].offset_x - offset_x;
                    let mut change_x = x as i32;
                    if vec_frame_part[index].mirror {
                        change_x = width  as i32 - 1 - x as i32;
                    }

                    if vec_rgba[index].get_pixel(change_x as u32, y)[3] != 0 {
                        rgba.put_pixel(new_x as u32,new_y as u32, *vec_rgba[index].get_pixel(change_x as u32, y)) ;

                    }
                }
            }
        }

        RgbaImageData{
            width:rgba.width() as u16,
            height:rgba.height() as u16,
            data:rgba.into_raw()
        }
    }

    fn image_save(image_new: RgbaImageData) {
        save_buffer(format!("image.png"), &image_new.data , image_new.width.into(), image_new.height.into(), image::ExtendedColorType::Rgba8).unwrap();
    }
}

impl AnimationData{
    pub fn render<'a>(
        &self,
        animation_state: &AnimationState,
        camera_direction: usize,
        head_direction: usize,
    ) -> (&Texture, bool) {
        let direction = (camera_direction + head_direction) % 8;
        let aa = animation_state.action * 8 + direction;
        let delay = 5.0;
        let animation = &self.animations[aa % self.animations.len()];

        let factor = animation_state
            .factor
            .map(|factor| delay * (factor / 5.0))
            .unwrap_or_else(|| delay * 50.0);

        let frame = animation_state
            .duration
            .map(|duration| animation_state.time * animation.textures.len() as u32 / duration)
            .unwrap_or_else(|| (animation_state.time as f32 / factor) as u32);
        // TODO: work out how to avoid losing digits when casting timg to an f32. When
        // fixed remove set_start_time in MouseCursor.
        let time = frame as usize % animation.textures.len();
        let texture = &animation.textures[time];
        let texture_size = texture.get_extend();
        (
            texture,
            false,
        )
    }
}



#[cfg(test)]
mod clip {
use super::*;
use std::fs;
/*
#[test]
    fn merge_test() {
        // Download every part of image

        let paths = fs::read_dir("./").unwrap();
        for path in paths {
            println!("Name: {}", path.unwrap().path().display())
        }

        let image_head = match image::ImageReader::open("./head.png") {
            Ok(image_reader) =>  match image_reader.decode() {
                Ok(image) => image,
                Err(e) => panic!("couldn't decode file"),
            },
            Err(err) => panic!("couldn't open image.png: {}", err),
        };

        let image_body = match image::ImageReader::open("./body.png") {
            Ok(image_reader) =>  match image_reader.decode() {
                Ok(image) => image,
                Err(e) => panic!("couldn't decode file"),
            },
            Err(err) => panic!("couldn't open image.png: {}", err),
        };        
        
        let head_offset_x = 1;
        let head_offset_y = -65;

        let body_offset_x = 1;
        let body_offset_y = -25;

        let head_height = image_head.height();
        let head_width = image_head.width();

        let body_height = image_body.height();
        let body_width = image_body.width();

        let mut frame_part: Vec<FramePart> = Vec::new();

        println!("{}", image_body.width());
        images.push(FramePart{
            width: image_body.width() as u16,
            height: image_body.height() as u16,
            data: image_body.to_rgba8().clone().into_raw(),
        });
        images.push(FramePart{
            width: image_head.width() as u16,
            height: image_head.height() as u16,
            data: image_head.to_rgba8().clone().into_raw(),
        });
       
        Frame::image_save(Frame::merge_get_animation_data(frame_part));
    } 
*/
}