mod vertices;

use std::collections::HashMap;
use std::sync::Arc;

use bytemuck::Pod;
use cgmath::{EuclideanSpace, Point3, Vector3};
use derive_new::new;
use korangar_audio::AudioEngine;
#[cfg(feature = "debug")]
use korangar_debug::logging::Timer;
use korangar_util::collision::{QuadTree, AABB};
use korangar_util::container::SimpleSlab;
use korangar_util::FileLoader;
use ragnarok_bytes::{ByteStream, FromBytes};
use ragnarok_formats::map::{GatData, GroundData, GroundTile, MapData, MapResources};
use ragnarok_formats::version::InternalVersion;
use wgpu::{BufferUsages, Device, Queue};

use self::vertices::{generate_tile_vertices, ground_water_vertices, load_textures};
use super::error::LoadError;
use crate::graphics::{Buffer, NativeModelVertex, Texture, TextureGroup};
use crate::loaders::{GameFileLoader, ModelLoader, TextureLoader};
use crate::world::*;

const MAP_OFFSET: f32 = 5.0;

#[cfg(feature = "debug")]
fn assert_byte_stream_empty<Meta>(mut byte_stream: ByteStream<Meta>, file_name: &str) {
    use korangar_debug::logging::{print_debug, Colorize};

    if !byte_stream.is_empty() {
        print_debug!(
            "incomplete read on file {}; {} bytes remaining",
            file_name.magenta(),
            byte_stream.remaining_bytes().len().yellow(),
        );
    }
}

#[derive(new)]
pub struct MapLoader {
    device: Arc<Device>,
    queue: Arc<Queue>,
    game_file_loader: Arc<GameFileLoader>,
    audio_engine: Arc<AudioEngine<GameFileLoader>>,
    #[new(default)]
    cache: HashMap<String, Arc<Map>>,
}

impl MapLoader {
    pub fn get(
        &mut self,
        resource_file: String,
        model_loader: &mut ModelLoader,
        texture_loader: &mut TextureLoader,
    ) -> Result<Arc<Map>, LoadError> {
        match self.cache.get(&resource_file) {
            Some(map) => Ok(map.clone()),
            None => self.load(resource_file, model_loader, texture_loader),
        }
    }

    fn load(
        &mut self,
        resource_file: String,
        model_loader: &mut ModelLoader,
        texture_loader: &mut TextureLoader,
    ) -> Result<Arc<Map>, LoadError> {
        #[cfg(feature = "debug")]
        let timer = Timer::new_dynamic(format!("load map from {}", &resource_file));
        
        let map_file_name = format!("data\\{}.rsw", resource_file);
        let mut map_data: MapData = parse_generic_data(&map_file_name, &self.game_file_loader)?;

        let ground_file = format!("data\\{}", map_data.ground_file);
        let ground_data: GroundData = parse_generic_data(&ground_file, &self.game_file_loader)?;

        let gat_file = format!("data\\{}", map_data.gat_file);
        let mut gat_data: GatData = parse_generic_data(&gat_file, &self.game_file_loader)?;

        #[cfg(feature = "debug")]
        let map_data_clone = map_data.clone();

        let (tile_vertices, tile_picker_vertices) = generate_tile_vertices(&mut gat_data);
        let water_level = -map_data
            .water_settings
            .as_ref()
            .and_then(|settings| settings.water_level)
            .unwrap_or_default();
        let (ground_vertices, water_vertices) = ground_water_vertices(&ground_data, water_level);

        let ground_vertices = NativeModelVertex::to_vertices(ground_vertices);

        let ground_vertex_buffer = self.create_vertex_buffer(&resource_file, "ground", &ground_vertices);
        let water_vertex_buffer = (!water_vertices.is_empty()).then(|| self.create_vertex_buffer(&resource_file, "water", &water_vertices));
        let tile_vertex_buffer = (!tile_vertices.is_empty()).then(|| self.create_vertex_buffer(&resource_file, "tile", &tile_vertices));
        let tile_picker_vertex_buffer =
            (!tile_picker_vertices.is_empty()).then(|| self.create_vertex_buffer(&resource_file, "tile picker", &tile_picker_vertices));

        let textures: Vec<Arc<Texture>> = load_textures(&ground_data, texture_loader);
        apply_map_offset(&ground_data, &mut map_data.resources);

        // Loading object models
        let map_width = ground_data.width as f32 * 10.0;
        let map_height = ground_data.height as f32 * 10.0;

        let mut objects = SimpleSlab::with_capacity(map_data.resources.objects.len() as u32);
        let mut quad_tree = QuadTree::new(
            AABB::new(Point3::new(0.0, -1000.0, 0.0), Point3::new(map_width, 1000.000, map_height)),
            5,
            5,
        );

        for object_data in map_data.resources.objects.iter() {
            let array: [f32; 3] = object_data.transform.scale.into();
            let reverse_order = array.into_iter().fold(1.0, |a, b| a * b).is_sign_negative();
            let model = model_loader
                .get(texture_loader, object_data.model_name.as_str(), reverse_order)
                .expect("can't find model");

            let object = Object::new(
                object_data.name.to_owned(),
                object_data.model_name.to_owned(),
                model,
                object_data.transform,
            );
            let bounding_box_matrix = object.get_bounding_box_matrix();
            let bounding_box = AABB::from_transformation_matrix(bounding_box_matrix);
            let key = objects.insert(object).expect("objects slab is full");

            quad_tree.insert(key, bounding_box);
        }
        let quad_tree = quad_tree.compact();

        self.set_ambient_sound_sources(&map_data, map_width, map_height);

        let textures = TextureGroup::new(&self.device, &map_file_name, textures);
        let bgm_track_name = self.audio_engine.get_track_for_map(&map_file_name);

        let map = Arc::new(Map::new(
            gat_data.map_width as usize,
            gat_data.map_height as usize,
            map_data.water_settings,
            map_data.light_settings,
            gat_data.tiles,
            ground_vertex_buffer,
            water_vertex_buffer,
            textures,
            objects,
            map_data.resources.light_sources,
            map_data.resources.sound_sources,
            map_data.resources.effect_sources,
            tile_picker_vertex_buffer.unwrap(),
            tile_vertex_buffer.unwrap(),
            quad_tree,
            bgm_track_name,
            #[cfg(feature = "debug")]
            map_data_clone,
        ));

        self.cache.insert(resource_file, map.clone());

        #[cfg(feature = "debug")]
        timer.stop();

        Ok(map)
    }

    fn set_ambient_sound_sources(&mut self, map_data: &MapData, map_width: f32, map_height: f32) {
        // This is the only correct place to clear the ambient sound.
        self.audio_engine.clear_ambient_sound();
        for sound in map_data.resources.sound_sources.iter() {
            let sfx_key = self.audio_engine.load(&sound.sound_file);
            self.audio_engine.add_ambient_sound(
                sfx_key,
                Point3::from_vec(sound.position),
                sound.range * 1.5, // We increase the range for a bit more ambient sound.
                sound.volume,
                sound.cycle,
            );
        }
        self.audio_engine.prepare_ambient_sound_world(map_width, map_height);
    }

    fn create_vertex_buffer<T: Pod>(&self, resource: &str, label: &str, vertices: &[T]) -> Buffer<T> {
        Buffer::with_data(
            &self.device,
            &self.queue,
            format!("{resource} {label}"),
            BufferUsages::COPY_DST | BufferUsages::VERTEX,
            vertices,
        )
    }
}

fn apply_map_offset(ground_data: &GroundData, resources: &mut MapResources) {
    let offset = Vector3::new(
        ground_data.width as f32 * MAP_OFFSET,
        0.0,
        ground_data.height as f32 * MAP_OFFSET,
    );

    resources.objects.iter_mut().for_each(|object| object.transform.position += offset);
    resources
        .sound_sources
        .iter_mut()
        .for_each(|sound_source| sound_source.offset(offset));
    resources
        .light_sources
        .iter_mut()
        .for_each(|light_source| light_source.offset(offset));
    resources
        .effect_sources
        .iter_mut()
        .for_each(|effect_source| effect_source.offset(offset));
}

fn parse_generic_data<Data: FromBytes>(resource_file: &str, game_file_loader: &GameFileLoader) -> Result<Data, LoadError> {
    let bytes = game_file_loader.get(resource_file).map_err(LoadError::File)?;
    let mut byte_stream: ByteStream<Option<InternalVersion>> = ByteStream::without_metadata(&bytes);

    let data = Data::from_bytes(&mut byte_stream).map_err(LoadError::Conversion)?;

    #[cfg(feature = "debug")]
    assert_byte_stream_empty(byte_stream, resource_file);

    Ok(data)
}

pub trait GroundTileExt {
    fn get_lowest_point(&self) -> f32;
}

impl GroundTileExt for GroundTile {
    fn get_lowest_point(&self) -> f32 {
        [
            self.lower_right_height,
            self.lower_left_height,
            self.upper_left_height,
            self.lower_right_height,
        ]
        .into_iter()
        .reduce(f32::max)
        .unwrap()
    }
}
