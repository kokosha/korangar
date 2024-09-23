//! This crate exposes an audio engine for the client
#![feature(let_chains)]
#![forbid(missing_docs)]

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::io::Cursor;
use std::mem::swap;
use std::num::{NonZeroU32, NonZeroUsize};
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cgmath::{InnerSpace, Matrix3, Point3, Quaternion, Vector3};
use cpal::BufferSize;
use kira::manager::backend::cpal::{CpalBackend, CpalBackendSettings};
use kira::manager::{AudioManager, AudioManagerSettings, Capacities};
use kira::sound::static_sound::{StaticSoundData, StaticSoundHandle};
use kira::sound::streaming::{StreamingSoundData, StreamingSoundHandle};
use kira::sound::{FromFileError, PlaybackState};
use kira::spatial::emitter::{EmitterDistances, EmitterHandle, EmitterSettings};
use kira::spatial::listener::{ListenerHandle, ListenerSettings};
use kira::spatial::scene::{SpatialSceneHandle, SpatialSceneSettings};
use kira::track::{TrackBuilder, TrackHandle};
use kira::tween::{Easing, Tween, Value};
use kira::{Frame, Volume};
#[cfg(feature = "debug")]
use korangar_debug::logging::{print_debug, Colorize, Timer};
use korangar_util::collision::{Compacted, QuadTree, Sphere, AABB};
use korangar_util::container::{Cacheable, GenerationalSlab, ResourceCache, SimpleSlab};
use korangar_util::{create_generational_key, create_simple_key, FileLoader};
use rayon::spawn;

create_generational_key!(SfxKey, "The key for a cached SFX");
create_simple_key!(AmbientKey, "The key for a ambient sound");

const MAX_QUEUE_TIME_SECONDS: f32 = 1.0;
const MAX_CACHE_COUNT: u32 = 400;
const MAX_CACHE_SIZE: usize = 50 * 104 * 1024; // 50 MiB
const SFX_BASE_PATH: &str = "data\\wav";
const BGM_MAPPING_FILE: &str = "data\\mp3NameTable.txt";
const AMBIENT_SOUND_WORLD_MAX_DEPTH: usize = 5;
const AMBIENT_SOUND_WORLD_SPILL_SIZE: usize = 5;

struct BgmTrack {
    track_name: String,
    handle: StreamingSoundHandle<FromFileError>,
}

struct QueuedSfx {
    /// The key of the sound that should be played.
    sfx_key: SfxKey,
    /// The optional key to the ambient sound emitter.
    ambient: Option<AmbientKey>,
    /// The time this playback was queued.
    queued_time: Instant,
}

struct AmbientSoundConfig {
    sfx_key: SfxKey,
    bounds: Sphere,
    volume: f32,
    cycle: Option<f32>,
}

struct PlayingAmbient {
    data: StaticSoundData,
    handle: StaticSoundHandle,
    cycle: f32,
    last_start: Instant,
}

#[repr(transparent)]
struct CachedSfx(StaticSoundData);

impl Cacheable for CachedSfx {
    fn size(&self) -> usize {
        self.0.frames.len() * size_of::<Frame>()
    }
}

enum AsyncLoadResult {
    Loaded {
        path: String,
        key: SfxKey,
        sfx: Box<StaticSoundData>,
    },
    Error {
        path: String,
        key: SfxKey,
        message: String,
    },
}

/// The audio engine of Korangar. Provides a simple interface to play BGM,
/// short sounds (SFX) and spatial, ambient sound (sounds on the map).
pub struct AudioEngine<FL> {
    engine_context: Mutex<EngineContext<FL>>,
}

struct EngineContext<FL> {
    active_emitters: HashMap<AmbientKey, EmitterHandle>,
    ambient_collision: QuadTree<AmbientKey, Sphere, Compacted>,
    ambient_listener: ListenerHandle,
    ambient_sound: SimpleSlab<AmbientKey, AmbientSoundConfig>,
    ambient_track: TrackHandle,
    async_response_receiver: Receiver<AsyncLoadResult>,
    async_response_sender: Sender<AsyncLoadResult>,
    bgm_track: TrackHandle,
    bgm_track_mapping: HashMap<String, String>,
    cache: ResourceCache<SfxKey, CachedSfx>,
    current_bgm_track: Option<BgmTrack>,
    cycling_ambient: HashMap<AmbientKey, PlayingAmbient>,
    game_file_loader: Arc<FL>,
    last_listener_update: Instant,
    loading_sfx: HashSet<SfxKey>,
    lookup: HashMap<String, SfxKey>,
    manager: AudioManager,
    previous_query_result: Vec<AmbientKey>,
    query_result: Vec<AmbientKey>,
    queued_bgm_track: Option<String>,
    queued_sfx: Vec<QueuedSfx>,
    scene: SpatialSceneHandle,
    scratchpad: Vec<AmbientKey>,
    sfx_paths: GenerationalSlab<SfxKey, String>,
    sfx_track: TrackHandle,
}

impl<FL: FileLoader> AudioEngine<FL> {
    /// Crates a new audio engine.
    pub fn new(game_file_loader: Arc<FL>) -> AudioEngine<FL> {
        let mut manager = AudioManager::<CpalBackend>::new(AudioManagerSettings {
            capacities: Capacities::default(),
            main_track_builder: TrackBuilder::default(),
            backend_settings: CpalBackendSettings {
                device: None,
                // At sampling rate of 48 kHz 1200 frames take 25 ms.
                buffer_size: BufferSize::Fixed(1200),
            },
        })
        .expect("Can't initialize audio backend");
        let mut scene = manager
            .add_spatial_scene(SpatialSceneSettings::default())
            .expect("Can't create spatial scene");
        let bgm_track = manager.add_sub_track(TrackBuilder::new()).expect("Can't create BGM track");
        let sfx_track = manager.add_sub_track(TrackBuilder::new()).expect("Can't create sfx track");
        let ambient_track = manager.add_sub_track(TrackBuilder::new()).expect("Can't create ambient track");
        let position = Vector3::new(0.0, 0.0, 0.0);
        let orientation = Quaternion::new(0.0, 0.0, 0.0, 0.0);
        let ambient_listener = scene
            .add_listener(position, orientation, ListenerSettings { track: ambient_track.id() })
            .expect("Can't create ambient listener");
        let loading_sfx = HashSet::new();
        let cache = ResourceCache::new(
            NonZeroU32::new(MAX_CACHE_COUNT).unwrap(),
            NonZeroUsize::new(MAX_CACHE_SIZE).unwrap(),
        );
        let (async_response_sender, async_response_receiver) = channel();

        let bgm_track_mapping = parse_bgm_track_mapping(game_file_loader.deref());

        let ambient_sound_world = QuadTree::new(
            AABB::uninitialized(),
            AMBIENT_SOUND_WORLD_MAX_DEPTH,
            AMBIENT_SOUND_WORLD_SPILL_SIZE,
        )
        .compact();

        let engine_context = Mutex::new(EngineContext {
            active_emitters: HashMap::default(),
            ambient_collision: ambient_sound_world,
            ambient_listener,
            ambient_sound: SimpleSlab::default(),
            ambient_track,
            async_response_receiver,
            async_response_sender,
            bgm_track,
            bgm_track_mapping,
            cache,
            current_bgm_track: None,
            cycling_ambient: HashMap::default(),
            game_file_loader,
            last_listener_update: Instant::now(),
            loading_sfx,
            lookup: HashMap::default(),
            manager,
            previous_query_result: Vec::default(),
            query_result: Vec::default(),
            queued_bgm_track: None,
            queued_sfx: Vec::default(),
            scene,
            scratchpad: Vec::default(),
            sfx_paths: GenerationalSlab::default(),
            sfx_track,
        });

        AudioEngine { engine_context }
    }

    /// This function needs the full file path with the file extension.
    pub fn get_track_for_map(&self, map_file_path: impl AsRef<Path>) -> Option<String> {
        let context = self.engine_context.lock().unwrap();

        let path: &Path = map_file_path.as_ref();
        let file_name = path.file_name()?.to_string_lossy();
        context.bgm_track_mapping.get(file_name.as_ref()).cloned()
    }

    /// Registers the given audio file path, queues it's loading and returns a
    /// key. If the audio file path was already registers, it will simply return
    /// it's key.
    pub fn load(&self, path: &str) -> SfxKey {
        let mut context = self.engine_context.lock().unwrap();

        if let Some(sfx_key) = context.lookup.get(path) {
            return *sfx_key;
        }

        let sfx_key = context.sfx_paths.insert(path.to_string()).expect("Mapping slab is full");
        context.lookup.insert(path.to_string(), sfx_key);

        spawn_async_load(
            context.game_file_loader.clone(),
            context.async_response_sender.clone(),
            path.to_string(),
            sfx_key,
        );

        sfx_key
    }

    /// Unloads und unregisters the registered audio file.
    pub fn unload(&self, sfx_key: SfxKey) {
        let mut context = self.engine_context.lock().unwrap();

        if let Some(path) = context.sfx_paths.remove(sfx_key) {
            let _ = context.lookup.remove(&path);
        }
        context.loading_sfx.remove(&sfx_key);
        let _ = context.cache.remove(sfx_key);
    }

    /// Sets the global volume.
    pub fn set_main_volume(&self, volume: impl Into<Value<Volume>>) {
        self.engine_context.lock().unwrap().set_main_volume(volume)
    }

    /// Sets the volume of the BGM.
    pub fn set_bgm_volume(&self, volume: impl Into<Value<Volume>>) {
        self.engine_context.lock().unwrap().set_bgm_volume(volume)
    }

    /// Sets the volume of SFX.
    pub fn set_sfx_volume(&self, volume: impl Into<Value<Volume>>) {
        self.engine_context.lock().unwrap().set_sfx_volume(volume)
    }

    /// Sets the volume of ambient sounds.
    pub fn set_ambient_volume(&self, volume: impl Into<Value<Volume>>) {
        self.engine_context.lock().unwrap().set_ambient_volume(volume)
    }

    /// Plays the BGM track. Fades out the currently playing BGM track and
    /// then start the new BGM track.
    pub fn play_bgm_track(&self, track_name: Option<&str>) {
        self.engine_context.lock().unwrap().play_bgm_track(track_name)
    }

    /// Plays an SFX.
    pub fn play_sfx(&self, sfx_key: SfxKey) {
        self.engine_context.lock().unwrap().play_sfx(sfx_key)
    }

    /// Sets the listener of the ambient sound. This is normally the camera's
    /// position and orientation. This should update each frame.
    pub fn set_ambient_listener(&self, position: Point3<f32>, view_direction: Vector3<f32>, look_up: Vector3<f32>) {
        self.engine_context
            .lock()
            .unwrap()
            .set_ambient_listener(position, view_direction, look_up)
    }

    /// Ambient sound loops and needs to be removed once the player it outside
    /// the ambient sound range.
    ///
    /// [`prepare_ambient_sound_world()`] must be called once all ambient sound
    /// have been added.
    pub fn add_ambient_sound(&self, sfx_key: SfxKey, position: Point3<f32>, range: f32, volume: f32, cycle: Option<f32>) -> AmbientKey {
        self.engine_context
            .lock()
            .unwrap()
            .add_ambient_sound(sfx_key, position, range, volume, cycle)
    }

    /// Removes an ambient sound.
    pub fn remove_ambient_sound(&self, ambient_key: AmbientKey) {
        self.engine_context.lock().unwrap().remove_ambient_sound(ambient_key)
    }

    /// Removes all ambient sound emitters from the spatial scene.
    pub fn clear_ambient_sound(&self) {
        self.engine_context.lock().unwrap().clear_ambient_sound()
    }

    /// Re-creates the spatial world with the ambient sounds.
    pub fn prepare_ambient_sound_world(&self, width: f32, height: f32) {
        self.engine_context.lock().unwrap().prepare_ambient_sound_world(width, height)
    }

    /// Updates the internal state of the audio engine. Should be called once
    /// each frame.
    pub fn update(&self) {
        self.engine_context.lock().unwrap().update()
    }
}

impl<FL: FileLoader> EngineContext<FL> {
    fn set_main_volume(&mut self, volume: impl Into<Value<Volume>>) {
        self.manager.main_track().set_volume(volume, Tween {
            duration: Duration::from_millis(500),
            ..Default::default()
        });
    }

    fn set_bgm_volume(&mut self, volume: impl Into<Value<Volume>>) {
        self.bgm_track.set_volume(volume, Tween {
            duration: Duration::from_millis(500),
            ..Default::default()
        });
    }

    fn set_sfx_volume(&mut self, volume: impl Into<Value<Volume>>) {
        self.sfx_track.set_volume(volume, Tween {
            duration: Duration::from_millis(500),
            ..Default::default()
        });
    }

    fn set_ambient_volume(&mut self, volume: impl Into<Value<Volume>>) {
        self.ambient_track.set_volume(volume, Tween {
            duration: Duration::from_millis(500),
            ..Default::default()
        });
    }

    fn play_bgm_track(&mut self, track_name: Option<&str>) {
        let Some(track_name) = track_name else {
            if let Some(playing) = self.current_bgm_track.as_mut() {
                playing.handle.stop(Tween {
                    duration: Duration::from_secs(1),
                    ..Default::default()
                });
            }
            self.current_bgm_track = None;
            return;
        };

        if let Some(playing) = self.current_bgm_track.as_mut()
            && (playing.handle.state() == PlaybackState::Playing || playing.handle.state() == PlaybackState::Stopping)
        {
            if playing.track_name.as_str() == track_name {
                return;
            }

            if playing.handle.state() == PlaybackState::Playing {
                playing.handle.stop(Tween {
                    duration: Duration::from_secs(1),
                    ..Default::default()
                });
            }

            self.queued_bgm_track = Some(track_name.to_string());
            return;
        }

        self.change_bgm_track(track_name);
    }

    fn play_sfx(&mut self, sfx_key: SfxKey) {
        if let Some(data) = self.cache.get(sfx_key).map(|cached_sfx| cached_sfx.0.clone()) {
            self.cache.touch(sfx_key);

            let data = data.output_destination(&self.sfx_track);
            if let Err(_err) = self.manager.play(data.clone()) {
                #[cfg(feature = "debug")]
                print_debug!("[{}] can't play SFX: {:?}", "error".red(), _err);
            }
            return;
        }

        queue_sfx_playback(
            self.game_file_loader.clone(),
            self.async_response_sender.clone(),
            &self.sfx_paths,
            &mut self.queued_sfx,
            sfx_key,
            None,
        );
    }

    fn set_ambient_listener(&mut self, position: Point3<f32>, view_direction: Vector3<f32>, look_up: Vector3<f32>) {
        let listener = Sphere::new(position, 10.0);
        self.query_result.clear();
        self.ambient_collision.query(&listener, &mut self.query_result);
        self.query_result.sort_unstable();

        // Add ambient sound that came into reach.
        difference(&mut self.query_result, &mut self.previous_query_result, &mut self.scratchpad);
        for ambient_key in self.scratchpad.iter().copied() {
            let Some(sound_config) = self.ambient_sound.get(ambient_key) else {
                #[cfg(feature = "debug")]
                print_debug!("[{}] can't find sound config for: {:?}", "error".red(), ambient_key);
                continue;
            };

            // Kira uses a RH coordinate system, so we need to convert our LH vectors.
            let position = sound_config.bounds.center();
            let position = Vector3::new(position.x, position.y, -position.z);
            let emitter_settings = EmitterSettings {
                distances: EmitterDistances {
                    min_distance: 5.0,
                    max_distance: sound_config.bounds.radius(),
                },
                attenuation_function: Some(Easing::Linear),
                enable_spatialization: true,
                persist_until_sounds_finish: false,
            };
            let emitter_handle = match self.scene.add_emitter(position, emitter_settings) {
                Ok(emitter_handle) => emitter_handle,
                Err(_err) => {
                    #[cfg(feature = "debug")]
                    print_debug!("[{}] can't add ambient sound emitter: {:?}", "error".red(), _err);
                    continue;
                }
            };

            let sfx_key = sound_config.sfx_key;
            if let Some(data) = self.cache.get(sfx_key).map(|cached_sfx| cached_sfx.0.clone()) {
                self.cache.touch(sfx_key);

                let data = adjust_ambient_sound(data, &emitter_handle, sound_config);
                match self.manager.play(data.clone()) {
                    Ok(handle) => {
                        if let Some(cycle) = sound_config.cycle {
                            self.cycling_ambient.insert(ambient_key, PlayingAmbient {
                                data,
                                handle,
                                cycle,
                                last_start: Instant::now(),
                            });
                        }
                    }
                    Err(_err) => {
                        #[cfg(feature = "debug")]
                        print_debug!("[{}] can't ambient SFX: {:?}", "error".red(), _err);
                    }
                }
            } else {
                queue_sfx_playback(
                    self.game_file_loader.clone(),
                    self.async_response_sender.clone(),
                    &self.sfx_paths,
                    &mut self.queued_sfx,
                    sfx_key,
                    Some(ambient_key),
                );
            }

            self.active_emitters.insert(ambient_key, emitter_handle);
        }

        // Remove ambient sound that are out of reach.
        difference(&mut self.previous_query_result, &mut self.query_result, &mut self.scratchpad);
        for ambient_key in self.scratchpad.iter().copied() {
            let _ = self.active_emitters.remove(&ambient_key);
            let _ = self.cycling_ambient.remove(&ambient_key);
        }

        // Update the previous result.
        swap(&mut self.query_result, &mut self.previous_query_result);

        // We only update the listener position once every 5ß ms, so that we can
        // properly ease the change and have no discontinuities.
        let now = Instant::now();
        if now.duration_since(self.last_listener_update).as_secs_f32() > 0.05 {
            self.last_listener_update = now;

            // Kira uses a RH coordinate system, so we need to convert our LH vectors.
            let position = Vector3::new(position.x, position.y, -position.z);
            let view_direction = Vector3::new(view_direction.x, view_direction.y, -view_direction.z).normalize();
            let look_up = Vector3::new(look_up.x, look_up.y, -look_up.z).normalize();
            let right = view_direction.cross(look_up).normalize();
            let up = right.cross(view_direction);

            let rotation_matrix = Matrix3::from_cols(right, up, -view_direction);
            let orientation = Quaternion::from(rotation_matrix);

            let tween = Tween {
                duration: Duration::from_millis(50),
                ..Default::default()
            };
            self.ambient_listener.set_position(position, tween);
            self.ambient_listener.set_orientation(orientation, tween);
        }
    }

    fn add_ambient_sound(&mut self, sfx_key: SfxKey, position: Point3<f32>, range: f32, volume: f32, cycle: Option<f32>) -> AmbientKey {
        self.ambient_sound
            .insert(AmbientSoundConfig {
                sfx_key,
                bounds: Sphere::new(position, range),
                volume,
                cycle,
            })
            .expect("Ambient sound slab is full")
    }

    fn remove_ambient_sound(&mut self, ambient_key: AmbientKey) {
        let _ = self.ambient_sound.remove(ambient_key);
        if let Some(emitter) = self.active_emitters.remove(&ambient_key) {
            // An emitter is removed from the spatial scene by dropping it. We make this
            // explicit to express our intent.
            drop(emitter);
        }
    }

    fn clear_ambient_sound(&mut self) {
        self.query_result.clear();
        self.previous_query_result.clear();
        self.scratchpad.clear();

        self.ambient_sound.clear();
        self.active_emitters.clear();
        self.cycling_ambient.clear();
    }

    fn prepare_ambient_sound_world(&mut self, width: f32, height: f32) {
        let mut new_world = QuadTree::new(
            AABB::new(Point3::new(0.0, -1000.0, 0.0), Point3::new(width, 1000.0, height)),
            AMBIENT_SOUND_WORLD_MAX_DEPTH,
            AMBIENT_SOUND_WORLD_SPILL_SIZE,
        );

        for (ambient_key, config) in self.ambient_sound.iter() {
            new_world.insert(ambient_key, config.bounds);
        }

        self.ambient_collision = new_world.compact();
    }

    fn update(&mut self) {
        self.resolve_async_loads();
        self.resolve_queued_audio();
        self.restart_cycling_ambient();
    }

    /// Audio engine will collect all static sfx data that finished loading.
    /// Should be called once a frame.
    fn resolve_async_loads(&mut self) {
        while let Ok(result) = self.async_response_receiver.try_recv() {
            match result {
                AsyncLoadResult::Loaded { path: _path, key, sfx } => {
                    self.loading_sfx.remove(&key);

                    if let Err(_err) = self.cache.insert(key, CachedSfx(*sfx)) {
                        #[cfg(feature = "debug")]
                        print_debug!(
                            "[{}] audio file is too big for cache. Path: '{}': {:?}",
                            "error".red(),
                            &_path,
                            _err
                        );
                    }
                }
                AsyncLoadResult::Error {
                    path: _path,
                    key,
                    message: _message,
                } => {
                    self.loading_sfx.remove(&key);
                    #[cfg(feature = "debug")]
                    print_debug!(
                        "[{}] could not load audio file. Path: '{}' : {}",
                        "error".red(),
                        _path,
                        _message
                    );
                }
            }
        }
    }

    fn resolve_queued_audio(&mut self) {
        if self.queued_bgm_track.is_some()
            && let Some(playing) = self.current_bgm_track.as_ref()
            && playing.handle.state() == PlaybackState::Stopped
        {
            let track_name = self.queued_bgm_track.take().unwrap();
            self.change_bgm_track(&track_name)
        }

        let now = Instant::now();

        self.queued_sfx.retain(|queued| {
            if queued.queued_time.duration_since(now).as_secs_f32() <= MAX_QUEUE_TIME_SECONDS {
                if let Some(data) = self.cache.get(queued.sfx_key).map(|cached_sfx| cached_sfx.0.clone()) {
                    match queued.ambient {
                        None => {
                            if let Err(_err) = self.manager.play(data.output_destination(&self.sfx_track)) {
                                #[cfg(feature = "debug")]
                                print_debug!("[{}] can't play SFX: {:?}", "error".red(), _err);
                            }
                        }
                        Some(ambient_key) => {
                            if let Some(emitter_handle) = self.active_emitters.get(&ambient_key)
                                && let Some(sound_config) = self.ambient_sound.get(ambient_key)
                            {
                                let data = adjust_ambient_sound(data, emitter_handle, sound_config);
                                match self.manager.play(data.clone()) {
                                    Ok(handle) => {
                                        if let Some(cycle) = sound_config.cycle {
                                            self.cycling_ambient.insert(ambient_key, PlayingAmbient {
                                                data,
                                                handle,
                                                cycle,
                                                last_start: Instant::now(),
                                            });
                                        }
                                    }
                                    Err(_err) => {
                                        #[cfg(feature = "debug")]
                                        print_debug!("[{}] can't ambient SFX: {:?}", "error".red(), _err);
                                    }
                                }
                            }
                        }
                    }
                    // We played or can't play it.
                    false
                } else {
                    // SFX not loaded yet.
                    true
                }
            } else {
                // We waited too long.
                false
            }
        });
    }

    fn restart_cycling_ambient(&mut self) {
        let now = Instant::now();
        for (_, playing) in self.cycling_ambient.iter_mut().filter(|(_, playing)| {
            playing.handle.state() != PlaybackState::Playing && now.duration_since(playing.last_start).as_secs_f32() >= playing.cycle
        }) {
            playing.last_start = now;

            match self.manager.play(playing.data.clone()) {
                Ok(handle) => {
                    playing.handle = handle;
                }
                Err(_err) => {
                    #[cfg(feature = "debug")]
                    print_debug!("[{}] can't ambient SFX: {:?}", "error".red(), _err);
                }
            }
        }
    }

    fn change_bgm_track(&mut self, track_name: &str) {
        #[cfg(feature = "debug")]
        let timer = Timer::new_dynamic(format!("change BGM track to {}", track_name.magenta()));

        let Some(path) = find_path(track_name) else {
            #[cfg(feature = "debug")]
            print_debug!("[{}] can't find BGM track: {:?}", "error".red(), track_name);
            return;
        };

        let data = match StreamingSoundData::from_file(path) {
            Ok(sfx_data) => sfx_data,
            Err(_err) => {
                #[cfg(feature = "debug")]
                print_debug!("[{}] can't decode BGM track: {:?}", "error".red(), _err);
                return;
            }
        };

        // TODO: NHA Remove volume offset once we have a proper volume control in place.
        let data = data
            .volume(Volume::Amplitude(0.1))
            .output_destination(&self.bgm_track)
            .loop_region(..);
        let handle = match self.manager.play(data) {
            Ok(handle) => handle,
            Err(_err) => {
                #[cfg(feature = "debug")]
                print_debug!("[{}] can't play BGM track: {:?}", "error".red(), _err);
                return;
            }
        };

        self.current_bgm_track = Some(BgmTrack {
            track_name: track_name.to_string(),
            handle,
        });

        #[cfg(feature = "debug")]
        timer.stop();
    }
}

fn adjust_ambient_sound(mut data: StaticSoundData, emitter_handle: &EmitterHandle, sound_config: &AmbientSoundConfig) -> StaticSoundData {
    // Kira does the volume mapping from linear to logarithmic for us.
    data.settings.volume = Volume::Amplitude(sound_config.volume as f64).into();
    data.output_destination(emitter_handle)
}

fn queue_sfx_playback(
    game_file_loader: Arc<impl FileLoader>,
    async_response_sender: Sender<AsyncLoadResult>,
    sfx_paths: &GenerationalSlab<SfxKey, String>,
    queued_sfx: &mut Vec<QueuedSfx>,
    sfx_key: SfxKey,
    ambient: Option<AmbientKey>,
) -> bool {
    let Some(path) = sfx_paths.get(sfx_key).cloned() else {
        // This case could happen, if the SFX was queued for deletion.
        return true;
    };

    queued_sfx.push(QueuedSfx {
        sfx_key,
        ambient,
        queued_time: Instant::now(),
    });

    spawn_async_load(game_file_loader, async_response_sender, path, sfx_key);
    false
}

/// Spawns a loading task on the standard thread pool.
fn spawn_async_load(game_file_loader: Arc<impl FileLoader>, async_response_sender: Sender<AsyncLoadResult>, path: String, key: SfxKey) {
    spawn(move || {
        // #[cfg(feature = "debug")]
        // let timer = Timer::new_dynamic(format!("load audio file from {}",
        // path.magenta()));

        let full_path = format!("{SFX_BASE_PATH}\\{path}");

        let data = match game_file_loader.get(&full_path) {
            Ok(data) => data,
            Err(err) => {
                let message = format!("can't find audio file: {err:?}");
                let _ = async_response_sender.send(AsyncLoadResult::Error { message, path, key });
                return;
            }
        };
        let sfx = match StaticSoundData::from_cursor(Cursor::new(data)) {
            Ok(sfx) => Box::new(sfx),
            Err(err) => {
                let message = format!("can't decode audio file: {err:?}");
                let _ = async_response_sender.send(AsyncLoadResult::Error { message, path, key });
                return;
            }
        };
        let _ = async_response_sender.send(AsyncLoadResult::Loaded { path, key, sfx });

        // #[cfg(feature = "debug")]
        // timer.stop();
    });
}

fn parse_bgm_track_mapping(game_file_loader: &impl FileLoader) -> HashMap<String, String> {
    let mut bgm_track_mapping: HashMap<String, String> = HashMap::new();

    match game_file_loader.get(BGM_MAPPING_FILE) {
        Ok(mapping_file_data) => {
            let content = String::from_utf8_lossy(&mapping_file_data);
            for line in content.lines() {
                if line.starts_with("//") {
                    continue;
                }
                let split: Vec<&str> = line.split('#').collect();
                if split.len() > 2 {
                    let resource_name = split[0].to_string();
                    let track_name = split[1].to_string();
                    bgm_track_mapping.insert(resource_name, track_name);
                }
            }
        }
        Err(_err) => {
            #[cfg(feature = "debug")]
            print_debug!("[{}] can't find BGM mapping file: {:?}", "error".red(), _err);
        }
    }

    bgm_track_mapping
}

fn find_path(track_name: &str) -> Option<PathBuf> {
    let lower_case = track_name.to_lowercase();
    let upper_case = track_name.replace("bgm", "BGM");

    let lower_case_path = PathBuf::from(&lower_case);
    let upper_case_path = PathBuf::from(&upper_case);

    let path = if upper_case_path.exists() {
        upper_case_path
    } else if lower_case_path.exists() {
        lower_case_path
    } else {
        return None;
    };

    Some(path)
}

fn difference<T: Ord + Copy>(vec1: &mut [T], vec2: &mut [T], result: &mut Vec<T>) {
    result.clear();

    let mut i = 0;
    let mut j = 0;

    while i < vec1.len() && j < vec2.len() {
        match vec1[i].cmp(&vec2[j]) {
            Ordering::Less => {
                result.push(vec1[i]);
                i += 1;
            }
            Ordering::Equal => {
                i += 1;
                j += 1;
            }
            Ordering::Greater => {
                j += 1;
            }
        }
    }

    result.extend_from_slice(&vec1[i..]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_difference() {
        let mut vec1 = vec![1, 3, 4, 6, 7];
        let mut vec2 = vec![2, 3, 5, 7, 8];
        let mut result = Vec::new();

        difference(&mut vec1, &mut vec2, &mut result);

        assert_eq!(result, vec![1, 4, 6]);
    }

    #[test]
    fn test_completely_different() {
        let mut vec1 = vec![1, 3, 5];
        let mut vec2 = vec![2, 4, 6];
        let mut result = Vec::new();

        difference(&mut vec1, &mut vec2, &mut result);

        assert_eq!(result, vec![1, 3, 5]);
    }

    #[test]
    fn test_one_empty_vector() {
        let mut vec1 = vec![1, 2, 3];
        let mut vec2: Vec<u32> = Vec::new();
        let mut result = Vec::new();

        difference(&mut vec1, &mut vec2, &mut result);

        assert_eq!(result, vec![1, 2, 3]);
    }

    #[test]
    fn test_no_difference() {
        let mut vec1 = vec![1, 2, 3];
        let mut vec2 = vec![1, 2, 3];
        let mut result = Vec::new();

        difference(&mut vec1, &mut vec2, &mut result);

        assert!(result.is_empty());
    }
}
