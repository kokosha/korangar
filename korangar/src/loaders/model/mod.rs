use std::sync::Arc;

use cgmath::{Array, EuclideanSpace, Matrix4, Point3, Rad, SquareMatrix, Vector2, Vector3};
use derive_new::new;
use hashbrown::{HashMap, HashSet};
#[cfg(feature = "debug")]
use korangar_debug::logging::{Colorize, Timer, print_debug};
use korangar_util::FileLoader;
use korangar_util::collision::AABB;
use korangar_util::math::multiply_matrix4_and_point3;
use korangar_util::texture_atlas::AllocationId;
use num::Zero;
use ragnarok_bytes::{ByteReader, FromBytes};
use ragnarok_formats::model::{ModelData, NodeData};
use ragnarok_formats::version::InternalVersion;
use smallvec::{SmallVec, smallvec};

use super::error::LoadError;
use super::{FALLBACK_MODEL_FILE, smooth_model_normals};
use crate::graphics::{Color, NativeModelVertex};
use crate::loaders::map::DeferredVertexGeneration;
use crate::loaders::{GameFileLoader, TextureAtlas, TextureAtlasEntry};
use crate::world::{Model, Node};

#[derive(new)]
pub struct ModelLoader {
    game_file_loader: Arc<GameFileLoader>,
}

impl ModelLoader {
    fn add_vertices(
        vertices: &mut [NativeModelVertex],
        vertex_positions: &[Point3<f32>],
        texture_coordinates: &[Vector2<f32>],
        smoothing_groups: &SmallVec<[i32; 3]>,
        texture_index: i32,
        reverse_vertices: bool,
        reverse_normal: bool,
    ) {
        let normal = match reverse_normal {
            true => NativeModelVertex::calculate_normal(vertex_positions[0], vertex_positions[1], vertex_positions[2]),
            false => NativeModelVertex::calculate_normal(vertex_positions[2], vertex_positions[1], vertex_positions[0]),
        };

        if reverse_vertices {
            for ((vertex_position, texture_coordinates), target) in vertex_positions
                .iter()
                .zip(texture_coordinates.iter())
                .rev()
                .zip(vertices.iter_mut())
            {
                *target = NativeModelVertex::new(
                    *vertex_position,
                    normal,
                    *texture_coordinates,
                    texture_index,
                    Color::WHITE,
                    0.0, // TODO: actually add wind affinity
                    smoothing_groups.clone(),
                );
            }
        } else {
            for ((vertex_position, texture_coordinates), target) in
                vertex_positions.iter().zip(texture_coordinates.iter()).zip(vertices.iter_mut())
            {
                *target = NativeModelVertex::new(
                    *vertex_position,
                    normal,
                    *texture_coordinates,
                    texture_index,
                    Color::WHITE,
                    0.0, // TODO: actually add wind affinity
                    smoothing_groups.clone(),
                );
            }
        }
    }

    fn make_vertices(node: &NodeData, main_matrix: &Matrix4<f32>, reverse_order: bool, smooth_normals: bool) -> Vec<NativeModelVertex> {
        let face_count = node.faces.len();
        let face_vertex_count = face_count * 3;
        let two_sided_face_count = node.faces.iter().filter(|face| face.two_sided != 0).count();
        let total_vertices = (face_count + two_sided_face_count) * 3;

        let mut native_vertices = vec![NativeModelVertex::zeroed(); total_vertices];
        let mut face_index = 0;
        let mut back_face_index = face_vertex_count;

        let array: [f32; 3] = node.scale.unwrap_or(Vector3::new(1.0, 1.0, 1.0)).into();
        let reverse_node_order = array.into_iter().fold(1.0, |a, b| a * b).is_sign_negative();

        if reverse_node_order {
            panic!("this can actually happen");
        }

        for face in &node.faces {
            let vertex_positions: [Point3<f32>; 3] = std::array::from_fn(|index| {
                let position_index = face.vertex_position_indices[index];
                let position = node.vertex_positions[position_index as usize];
                multiply_matrix4_and_point3(main_matrix, position)
            });

            let texture_coordinates: [Vector2<f32>; 3] = std::array::from_fn(|index| {
                let coordinate_index = face.texture_coordinate_indices[index];
                node.texture_coordinates[coordinate_index as usize].coordinates
            });
            let mut smoothing_groups: SmallVec<[i32; 3]> = smallvec![face.smooth_group];
            if let Some(extras) = face.smooth_group_extra.as_ref() {
                for extra in extras {
                    smoothing_groups.push(*extra);
                }
            };
            Self::add_vertices(
                &mut native_vertices[face_index..face_index + 3],
                &vertex_positions,
                &texture_coordinates,
                &smoothing_groups,
                face.texture_index as i32,
                reverse_order,
                false,
            );
            face_index += 3;

            if face.two_sided != 0 {
                Self::add_vertices(
                    &mut native_vertices[back_face_index..back_face_index + 3],
                    &vertex_positions,
                    &texture_coordinates,
                    &smoothing_groups,
                    face.texture_index as i32,
                    !reverse_order,
                    true,
                );
                back_face_index += 3;
            }
        }

        if smooth_normals {
            let (face_vertices, back_face_vertices) = native_vertices.split_at_mut(face_vertex_count);
            smooth_model_normals(face_vertices);
            smooth_model_normals(back_face_vertices);
        }

        native_vertices
    }

    fn calculate_matrices_rsm1(node: &NodeData, parent_matrix: &Matrix4<f32>) -> (Matrix4<f32>, Matrix4<f32>, Matrix4<f32>) {
        let main = Matrix4::from_translation(node.translation1.unwrap_or(Vector3::zero())) * Matrix4::from(node.offset_matrix);
        let scale = node.scale.unwrap_or(Vector3::from_value(1.0));
        let scale_matrix = Matrix4::from_nonuniform_scale(scale.x, scale.y, scale.z);
        let rotation_matrix = Matrix4::from_axis_angle(
            node.rotation_axis.unwrap_or(Vector3::zero()),
            Rad(node.rotation_angle.unwrap_or(0.0)),
        );
        let translation_matrix = Matrix4::from_translation(node.translation2);

        let transform = match node.rotation_keyframe_count > 0 {
            true => translation_matrix * scale_matrix,
            false => translation_matrix * rotation_matrix * scale_matrix,
        };

        let box_transform = parent_matrix * translation_matrix * rotation_matrix * scale_matrix;

        (main, transform, box_transform)
    }

    fn calculate_matrices_rsm2(node: &NodeData) -> (Matrix4<f32>, Matrix4<f32>, Matrix4<f32>) {
        let main = Matrix4::identity();
        let translation_matrix = Matrix4::from_translation(node.translation2);
        let transform = translation_matrix * Matrix4::from(node.offset_matrix);
        let box_transform = transform;

        (main, transform, box_transform)
    }

    fn calculate_centroid(vertices: &[NativeModelVertex]) -> Point3<f32> {
        let sum = vertices.iter().fold(Vector3::new(0.0, 0.0, 0.0), |accumulator, vertex| {
            accumulator + vertex.position.to_vec()
        });
        Point3::from_vec(sum / vertices.len() as f32)
    }

    fn process_node_mesh(
        version: InternalVersion,
        current_node: &NodeData,
        nodes: &[NodeData],
        processed_node_indices: &mut [bool],
        vertex_offset: &mut usize,
        native_vertices: &mut Vec<NativeModelVertex>,
        texture_mapping: &TextureMapping,
        parent_matrix: &Matrix4<f32>,
        main_bounding_box: &mut AABB,
        reverse_order: bool,
        smooth_normals: bool,
        frames_per_second: f32,
        animation_length: u32,
    ) -> Node {
        let (main_matrix, transform_matrix, box_transform_matrix) = match version.equals_or_above(2, 2) {
            false => Self::calculate_matrices_rsm1(current_node, parent_matrix),
            true => Self::calculate_matrices_rsm2(current_node),
        };

        let rotation_matrix = current_node.offset_matrix;
        let position = current_node.translation2.extend(0.0);

        let box_matrix = box_transform_matrix * main_matrix;
        let bounding_box = AABB::from_vertices(
            current_node
                .vertex_positions
                .iter()
                .map(|position| multiply_matrix4_and_point3(&box_matrix, *position)),
        );
        main_bounding_box.extend(&bounding_box);

        let child_indices: Vec<usize> = nodes
            .iter()
            .enumerate()
            .filter(|&(index, node)| {
                node.parent_node_name == current_node.node_name && !std::mem::replace(&mut processed_node_indices[index], true)
            })
            .map(|(i, _)| i)
            .collect();

        let child_nodes: Vec<Node> = child_indices
            .iter()
            .map(|&index| {
                Self::process_node_mesh(
                    version,
                    &nodes[index],
                    nodes,
                    processed_node_indices,
                    vertex_offset,
                    native_vertices,
                    texture_mapping,
                    &box_transform_matrix,
                    main_bounding_box,
                    reverse_order,
                    smooth_normals,
                    frames_per_second,
                    animation_length,
                )
            })
            .collect();

        // Map the node texture index to the model texture index.
        let (node_texture_mapping, texture_transparency): (Vec<i32>, Vec<bool>) = match texture_mapping {
            TextureMapping::PreVersion2_3(vector_texture) => current_node
                .texture_indices
                .iter()
                .map(|&index| {
                    let model_texture = vector_texture[index as usize];
                    (model_texture.index, model_texture.transparent)
                })
                .unzip(),
            TextureMapping::PostVersion2_3(hashmap_texture) => current_node
                .texture_names
                .iter()
                .map(|name| {
                    let model_texture = hashmap_texture.get(name.as_ref()).unwrap();
                    (model_texture.index, model_texture.transparent)
                })
                .unzip(),
        };

        let has_transparent_parts = texture_transparency.iter().any(|x| *x);

        let mut node_native_vertices = Self::make_vertices(current_node, &main_matrix, reverse_order, smooth_normals);
        let centroid = Self::calculate_centroid(&node_native_vertices);

        node_native_vertices
            .iter_mut()
            .for_each(|vertice| vertice.texture_index = node_texture_mapping[vertice.texture_index as usize]);

        // Remember the vertex offset/count and gather node vertices.
        let node_vertex_offset = *vertex_offset;
        let node_vertex_count = node_native_vertices.len();
        *vertex_offset += node_vertex_count;
        native_vertices.extend(node_native_vertices.iter().cloned());

        // Apply the frames per second on the keyframes values.
        let animation_length = match version.equals_or_above(2, 2) {
            true => (animation_length as f32 * 1000.0 / frames_per_second).floor() as u32,
            false => animation_length,
        };

        let scale_keyframes = match version.equals_or_above(2, 2) {
            true => {
                let mut scale_keyframes = current_node.scale_keyframes.clone();
                for data in scale_keyframes.iter_mut() {
                    data.frame = (data.frame as f32 * 1000.0 / frames_per_second).floor() as i32;
                }
                scale_keyframes
            }
            false => current_node.scale_keyframes.clone(),
        };

        let translation_keyframes = match version.equals_or_above(2, 2) {
            true => {
                let mut translation_keyframes = current_node.translation_keyframes.clone();
                for data in translation_keyframes.iter_mut() {
                    data.frame = (data.frame as f32 * 1000.0 / frames_per_second).floor() as i32;
                }
                translation_keyframes
            }
            false => current_node.translation_keyframes.clone(),
        };

        let rotation_keyframes = match version.equals_or_above(2, 2) {
            true => {
                let mut rotation_keyframes = current_node.rotation_keyframes.clone();
                for data in rotation_keyframes.iter_mut() {
                    data.frame = (data.frame as f32 * 1000.0 / frames_per_second).floor() as i32;
                }
                rotation_keyframes
            }
            false => current_node.rotation_keyframes.clone(),
        };

        Node::new(
            version,
            transform_matrix,
            rotation_matrix.into(),
            Matrix4::<f32>::identity(),
            position,
            centroid,
            has_transparent_parts,
            node_vertex_offset,
            node_vertex_count,
            child_nodes,
            animation_length,
            scale_keyframes,
            translation_keyframes,
            rotation_keyframes,
        )
    }

    pub fn calculate_transformation_matrix(
        node: &mut Node,
        is_root: bool,
        bounding_box: AABB,
        parent_matrix: &Matrix4<f32>,
        parent_rotation_matrix: &Matrix4<f32>,
        is_static: bool,
    ) {
        let transform_matrix = match is_root {
            true => {
                let translation_matrix = Matrix4::from_translation(-Vector3::new(
                    bounding_box.center().x,
                    bounding_box.max().y,
                    bounding_box.center().z,
                ));
                match node.version.equals_or_above(2, 2) {
                    true => node.transform_matrix,
                    false => translation_matrix * node.transform_matrix,
                }
            }
            false => node.transform_matrix,
        };

        match node.version.equals_or_above(2, 2) {
            false => {
                node.transform_matrix = match is_static {
                    true => parent_matrix * transform_matrix,
                    false => transform_matrix,
                };
            }
            true => node.parent_rotation_matrix = *parent_rotation_matrix,
        }

        node.child_nodes.iter_mut().for_each(|child_node| {
            Self::calculate_transformation_matrix(
                child_node,
                false,
                bounding_box,
                &node.transform_matrix,
                &node.rotation_matrix,
                is_static,
            );
        });
    }

    // A model is static, if it doesn't have any animations.
    pub fn is_static(node: &Node) -> bool {
        node.scale_keyframes.is_empty()
            && node.translation_keyframes.is_empty()
            && node.rotation_keyframes.is_empty()
            && node.child_nodes.iter().all(Self::is_static)
    }

    /// We need to make sure to always generate a texture atlas in the same
    /// order when creating an online texture atlas and an offline texture
    /// atlas.
    fn collect_versioned_texture_names(version: &InternalVersion, model_data: &ModelData) -> Vec<String> {
        match version.equals_or_above(2, 3) {
            false => model_data
                .texture_names
                .iter()
                .map(|texture_name| texture_name.inner.clone())
                .collect(),
            true => {
                let mut hashset = HashSet::<String>::new();
                let mut result = Vec::<String>::with_capacity(5);
                model_data.nodes.iter().for_each(|node_data| {
                    node_data.texture_names.iter().for_each(|name| {
                        let inner_name = &name.inner;
                        if !hashset.contains(inner_name) {
                            hashset.insert(inner_name.clone());
                            result.push(name.inner.clone());
                        }
                    })
                });
                result
            }
        }
    }

    pub fn collect_model_textures(&self, textures: &mut HashSet<String>, model_file: &str) {
        let Ok(bytes) = self.game_file_loader.get(&format!("data\\model\\{model_file}")) else {
            return;
        };
        let mut byte_reader: ByteReader<Option<InternalVersion>> = ByteReader::with_default_metadata(&bytes);

        let Ok(model_data) = ModelData::from_bytes(&mut byte_reader) else {
            return;
        };

        let version: InternalVersion = model_data.version.into();

        let texture_names = ModelLoader::collect_versioned_texture_names(&version, &model_data);
        texture_names.into_iter().for_each(|texture_name| {
            let _ = textures.insert(texture_name);
        });
    }

    pub fn load(
        &self,
        texture_atlas: &mut dyn TextureAtlas,
        vertex_offset: &mut usize,
        model_file: &str,
        reverse_order: bool,
    ) -> Result<(Model, DeferredVertexGeneration), LoadError> {
        #[cfg(feature = "debug")]
        let timer = Timer::new_dynamic(format!("load rsm model from {}", model_file.magenta()));

        let bytes = match self.game_file_loader.get(&format!("data\\model\\{model_file}")) {
            Ok(bytes) => bytes,
            Err(_error) => {
                #[cfg(feature = "debug")]
                {
                    print_debug!("Failed to load model: {:?}", _error);
                    print_debug!("Replacing with fallback");
                }

                return self.load(texture_atlas, vertex_offset, FALLBACK_MODEL_FILE, reverse_order);
            }
        };
        let mut byte_reader: ByteReader<Option<InternalVersion>> = ByteReader::with_default_metadata(&bytes);

        let model_data = match ModelData::from_bytes(&mut byte_reader) {
            Ok(model_data) => model_data,
            Err(_error) => {
                #[cfg(feature = "debug")]
                {
                    print_debug!("Failed to load model: {:?}", _error);
                    print_debug!("Replacing with fallback");
                }

                return self.load(texture_atlas, vertex_offset, FALLBACK_MODEL_FILE, reverse_order);
            }
        };

        // TODO: Temporary check until we support more versions.
        // TODO: The model operation to modify texture keyframe is not implemented yet.
        let version: InternalVersion = model_data.version.into();
        if version.equals_or_above(2, 4) {
            #[cfg(feature = "debug")]
            {
                print_debug!("Failed to load model because version {} is unsupported", version);
                print_debug!("Replacing with fallback");
            }

            return self.load(texture_atlas, vertex_offset, FALLBACK_MODEL_FILE, reverse_order);
        }

        let texture_names = ModelLoader::collect_versioned_texture_names(&version, &model_data);

        let texture_allocation: Vec<TextureAtlasEntry> = texture_names
            .iter()
            .map(|texture_name| texture_atlas.register(texture_name.as_ref()))
            .collect();

        let texture_mapping = match version.equals_or_above(2, 3) {
            true => {
                let hashmap_texture =
                    HashMap::<String, ModelTexture>::from_iter(texture_names.into_iter().zip(texture_allocation.clone()).enumerate().map(
                        |(index, (name, entry))| {
                            (name, ModelTexture {
                                index: index as i32,
                                transparent: entry.transparent,
                            })
                        },
                    ));
                TextureMapping::PostVersion2_3(hashmap_texture)
            }
            false => {
                let vector_texture: Vec<ModelTexture> = texture_allocation
                    .iter()
                    .enumerate()
                    .map(|(index, entry)| ModelTexture {
                        index: index as i32,
                        transparent: entry.transparent,
                    })
                    .collect();
                TextureMapping::PreVersion2_3(vector_texture)
            }
        };

        let root_node_names = match version.equals_or_above(2, 2) {
            true => model_data.root_node_names.to_vec(),
            false => vec![model_data.root_node_name.clone().unwrap()],
        };

        let root_info: Vec<(usize, &NodeData)> = root_node_names
            .iter()
            .map(|node_name| {
                let (root_node_position, root_node) = model_data
                    .nodes
                    .iter()
                    .enumerate()
                    .find(|(_, node_data)| node_data.node_name == *node_name)
                    .expect("failed to find main node");
                (root_node_position, root_node)
            })
            .collect();

        let mut processed_node_indices = vec![false; model_data.nodes.len()];
        let mut model_bounding_box = AABB::uninitialized();
        let mut native_model_vertices = Vec::<NativeModelVertex>::new();

        let mut root_nodes: Vec<Node> = root_info
            .into_iter()
            .map(|(root_node_position, root_node)| {
                processed_node_indices[root_node_position] = true;
                Self::process_node_mesh(
                    version,
                    root_node,
                    &model_data.nodes,
                    &mut processed_node_indices,
                    vertex_offset,
                    &mut native_model_vertices,
                    &texture_mapping,
                    &Matrix4::identity(),
                    &mut model_bounding_box,
                    reverse_order ^ version.equals_or_above(2, 2),
                    model_data.shade_type == 2,
                    model_data.frames_per_second.unwrap_or(60.0),
                    model_data.animation_length,
                )
            })
            .collect();

        drop(texture_mapping);

        let is_static = root_nodes.iter().all(Self::is_static);

        for root_node in root_nodes.iter_mut() {
            Self::calculate_transformation_matrix(
                root_node,
                true,
                model_bounding_box,
                &Matrix4::identity(),
                &Matrix4::identity(),
                is_static,
            );
        }

        let model = Model::new(
            version,
            root_nodes,
            model_bounding_box,
            is_static,
            #[cfg(feature = "debug")]
            model_data,
        );

        let texture_allocation: Vec<AllocationId> = texture_allocation.iter().map(|entry| entry.allocation_id).collect();

        let deferred = DeferredVertexGeneration {
            native_model_vertices,
            texture_allocation,
        };

        #[cfg(feature = "debug")]
        timer.stop();

        Ok((model, deferred))
    }
}

#[derive(Copy, Clone)]
struct ModelTexture {
    index: i32,
    transparent: bool,
}

enum TextureMapping {
    PreVersion2_3(Vec<ModelTexture>),
    PostVersion2_3(HashMap<String, ModelTexture>),
}
