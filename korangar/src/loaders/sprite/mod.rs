use std::collections::HashMap;
use std::sync::Arc;

use derive_new::new;
use image::RgbaImage;
#[cfg(feature = "debug")]
use korangar_debug::logging::{print_debug, Colorize, Timer};
use korangar_interface::elements::PrototypeElement;
use korangar_util::texture_atlas::{AllocationId, AtlasAllocation, TextureAtlas};
use korangar_util::FileLoader;
use ragnarok_bytes::{ByteStream, FromBytes};
use ragnarok_formats::sprite::{PaletteColor, RgbaImageData, SpriteData};
use ragnarok_formats::version::InternalVersion;
use wgpu::{Device, Extent3d, Queue, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages};

use super::FALLBACK_SPRITE_FILE;
use crate::graphics::Texture;
use crate::loaders::error::LoadError;
use crate::loaders::GameFileLoader;

#[derive(Clone, Debug, PrototypeElement)]
pub struct Sprite {
    pub palette_size: usize,
    #[hidden_element]
    pub textures: Vec<Arc<Texture>>,
    #[cfg(feature = "debug")]
    sprite_data: SpriteData,
}

#[derive(new)]
pub struct SpriteLoader {
    device: Arc<Device>,
    queue: Arc<Queue>,
    game_file_loader: Arc<GameFileLoader>,
    #[new(default)]
    cache: HashMap<String, Arc<Sprite>>,
    #[new(default)]
    cache_atlas: HashMap<String, Arc<SpriteAtlas>>,
}

impl SpriteLoader {
    fn load(&mut self, path: &str) -> Result<Arc<Sprite>, LoadError> {
        #[cfg(feature = "debug")]
        let timer = Timer::new_dynamic(format!("load sprite from {}", path.magenta()));

        let bytes = self
            .game_file_loader
            .get(&format!("data\\sprite\\{path}"))
            .map_err(LoadError::File)?;
        let mut byte_stream: ByteStream<Option<InternalVersion>> = ByteStream::without_metadata(&bytes);

        let sprite_data = match SpriteData::from_bytes(&mut byte_stream) {
            Ok(sprite_data) => sprite_data,
            Err(_error) => {
                #[cfg(feature = "debug")]
                {
                    print_debug!("Failed to load sprite: {:?}", _error);
                    print_debug!("Replacing with fallback");
                }

                return self.get(FALLBACK_SPRITE_FILE);
            }
        };

        #[cfg(feature = "debug")]
        let cloned_sprite_data = sprite_data.clone();

        let palette = sprite_data.palette.unwrap(); // unwrap_or_default() as soon as i know what

        let rgba_images: Vec<RgbaImageData> = sprite_data
            .rgba_image_data
            .iter()
            .map(|image_data| {
                // Revert the rows, the image is flipped upside down
                // Convert the pixel from ABGR format to RGBA format
                let width = image_data.width;
                let mut data: Vec<u8> = image_data.data.clone();
                data = data
                    .chunks_exact(4 * width as usize)
                    .rev()
                    .flat_map(|pixels| {
                        pixels
                            .chunks_exact(4)
                            .flat_map(|pixel| [pixel[3], pixel[2], pixel[1], pixel[0]])
                            .collect::<Vec<u8>>()
                    })
                    .collect();

                RgbaImageData {
                    width: image_data.width,
                    height: image_data.height,
                    data,
                }
            })
            .collect();

        // TODO: Move this to an extension trait in `korangar_loaders`.
        pub fn color_bytes(palette: &PaletteColor, index: u8) -> [u8; 4] {
            let alpha = match index {
                0 => 0,
                _ => 255,
            };

            [palette.red, palette.green, palette.blue, alpha]
        }

        let palette_images = sprite_data.palette_image_data.iter().map(|image_data| {
            // Decode palette image data if necessary
            let data: Vec<u8> = image_data
                .data
                .0
                .iter()
                .flat_map(|palette_index| color_bytes(&palette.colors[*palette_index as usize], *palette_index))
                .collect();

            RgbaImageData {
                width: image_data.width,
                height: image_data.height,
                data,
            }
        });
        let palette_size = palette_images.len();

        let textures = palette_images
            .chain(rgba_images)
            .map(|image_data| {
                let texture = Texture::new_with_data(
                    &self.device,
                    &self.queue,
                    &TextureDescriptor {
                        label: Some(path),
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

        let sprite = Arc::new(Sprite {
            palette_size,
            textures,
            #[cfg(feature = "debug")]
            sprite_data: cloned_sprite_data,
        });
        self.cache.insert(path.to_string(), sprite.clone());

        #[cfg(feature = "debug")]
        timer.stop();

        Ok(sprite)
    }

    pub fn get(&mut self, path: &str) -> Result<Arc<Sprite>, LoadError> {
        match self.cache.get(path) {
            Some(sprite) => Ok(sprite.clone()),
            None => self.load(path),
        }
    }

    pub fn load_atlas(&self, path: &str) -> Result<Arc<SpriteAtlas>, LoadError> {
        #[cfg(feature = "debug")]
        let timer = Timer::new_dynamic(format!("load sprite from {}", path.magenta()));

        let bytes = self
            .game_file_loader
            .get(&format!("data\\sprite\\{path}"))
            .map_err(LoadError::File)?;
        let mut byte_stream: ByteStream<Option<InternalVersion>> = ByteStream::without_metadata(&bytes);

        let sprite_data = match SpriteData::from_bytes(&mut byte_stream) {
            Ok(sprite_data) => sprite_data,
            Err(_error) => {
                #[cfg(feature = "debug")]
                {
                    print_debug!("Failed to load sprite: {:?}", _error);
                    print_debug!("Replacing with fallback");
                }
                return self.load_atlas(FALLBACK_SPRITE_FILE);
            }
        };

        let palette = sprite_data.palette.unwrap(); // unwrap_or_default() as soon as i know what

        let rgba_images: Vec<RgbaImageData> = sprite_data
            .rgba_image_data
            .iter()
            .map(|image_data| {
                // Revert the rows, the image is flipped upside down
                // Convert the pixel from ABGR format to RGBA format
                let width = image_data.width;
                let mut data: Vec<u8> = image_data.data.clone();
                data = data
                    .chunks_exact(4 * width as usize)
                    .rev()
                    .flat_map(|pixels| {
                        pixels
                            .chunks_exact(4)
                            .flat_map(|pixel| [pixel[3], pixel[2], pixel[1], pixel[0]])
                            .collect::<Vec<u8>>()
                    })
                    .collect();

                RgbaImageData {
                    width: image_data.width,
                    height: image_data.height,
                    data,
                }
            })
            .collect();

        // TODO: Move this to an extension trait in `korangar_loaders`.
        pub fn color_bytes(palette: &PaletteColor, index: u8) -> [u8; 4] {
            let alpha = match index {
                0 => 0,
                _ => 255,
            };

            [palette.red, palette.green, palette.blue, alpha]
        }

        let palette_images = sprite_data.palette_image_data.iter().map(|image_data| {
            // Decode palette image data if necessary
            let data: Vec<u8> = image_data
                .data
                .0
                .iter()
                .flat_map(|palette_index| color_bytes(&palette.colors[*palette_index as usize], *palette_index))
                .collect();

            RgbaImageData {
                width: image_data.width,
                height: image_data.height,
                data,
            }
        });

        let rgba_images = palette_images.chain(rgba_images).map(|x| x).collect();

        let mut factory = SpriteAtlasFactory::new(path, true);
        factory.load(rgba_images);
        factory.build_atlas();

        let atlas_allocation = factory.get_atlas_allocation();

        // Create texture atlas
        let texture = self.create_texture(&factory.name.clone(), factory.get_atlas());

        let sprite_atlas = SpriteAtlas { texture, atlas_allocation };
        let result = Arc::new(sprite_atlas);

        #[cfg(feature = "debug")]
        timer.stop();

        Ok(result)
    }

    pub fn get_atlas(&self, path: &str) -> Result<Arc<SpriteAtlas>, LoadError> {
        match self.cache_atlas.get(path) {
            Some(sprite_atlas) => Ok(sprite_atlas.clone()),
            None => self.load_atlas(path),
        }
    }

    pub fn create_texture(&self, name: &str, image: RgbaImage) -> Arc<Texture> {
        let texture = Texture::new_with_data(
            &self.device,
            &self.queue,
            &TextureDescriptor {
                label: Some(name),
                size: Extent3d {
                    width: image.width(),
                    height: image.height(),
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: TextureFormat::Rgba8UnormSrgb,
                usage: TextureUsages::COPY_DST | TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            image.as_raw(),
        );
        Arc::new(texture)
    }
}
// Each sprite data will have their own atlas
pub struct SpriteAtlas {
    pub atlas_allocation: Vec<AtlasAllocation>,
    pub texture: Arc<Texture>,
}
pub struct SpriteAtlasFactory {
    name: String,
    texture_atlas: TextureAtlas,
    lookup: Vec<AllocationId>,
}

impl SpriteAtlasFactory {
    pub fn new(name: impl Into<String>, add_padding: bool) -> SpriteAtlasFactory {
        SpriteAtlasFactory {
            name: name.into(),
            texture_atlas: TextureAtlas::new(add_padding),
            lookup: Vec::default(),
        }
    }

    pub fn load(&mut self, datas: Vec<RgbaImageData>) {
        for data in datas.iter() {
            self.register(data.clone());
        }
    }

    pub fn register(&mut self, data: RgbaImageData) {
        let rgba_data = RgbaImage::from_raw(data.width as u32, data.height as u32, data.data).unwrap();
        let allocation_id = self.texture_atlas.register_image(rgba_data);
        self.lookup.push(allocation_id);
    }

    pub fn get_atlas_allocation(&self) -> Vec<AtlasAllocation> {
        self.lookup
            .iter()
            .map(|allocation_id| self.texture_atlas.get_allocation(*allocation_id).unwrap())
            .collect()
    }

    // Use this function after insert all images
    pub fn build_atlas(&mut self) {
        self.texture_atlas.build_atlas();
    }

    // After this function the factory is not usable anymore
    pub fn get_atlas(self) -> RgbaImage {
        self.texture_atlas.get_atlas()
    }
}
