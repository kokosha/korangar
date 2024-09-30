use std::fmt;
use std::string::String;
use std::sync::Arc;

use cgmath::{Array, Vector2, Vector3, VectorSpace};
use derive_new::new;
use image::{save_buffer, RgbaImage};
use korangar_interface::elements::PrototypeElement;
use korangar_interface::windows::{PrototypeWindow, Window};
use korangar_networking::EntityData;
use ragnarok_formats::map::TileFlags;
use ragnarok_formats::sprite::RgbaImageData;
use ragnarok_packets::{AccountId, CharacterInformation, ClientTick, EntityId, Sex, StatusType, WorldPosition};
#[cfg(feature = "debug")]
use wgpu::Buffer;
use wgpu::RenderPass;

#[cfg(feature = "debug")]
use crate::graphics::MarkerRenderer;
use crate::graphics::{Camera, DeferredRenderer, EntityRenderer, Renderer};
use crate::interface::application::InterfaceSettings;
use crate::interface::layout::{ScreenPosition, ScreenSize};
use crate::interface::theme::GameTheme;
use crate::interface::windows::WindowCache;
use crate::loaders::{
    ActionLoader, Actions, AnimationData, AnimationLoader, AnimationPair, AnimationState, GameFileLoader, ScriptLoader, Sprite,
    SpriteLoader,
};
use crate::world::Map;
#[cfg(feature = "debug")]
use crate::world::MarkerIdentifier;

pub enum ResourceState<T> {
    Available(T),
    Unavailable,
    Requested,
}

impl<T> ResourceState<T> {
    pub fn as_option(&self) -> Option<&T> {
        match self {
            ResourceState::Available(value) => Some(value),
            _requested_or_unavailable => None,
        }
    }
}

#[derive(Clone, new, PrototypeElement)]
pub struct Movement {
    #[hidden_element]
    steps: Vec<(Vector2<usize>, u32)>,
    starting_timestamp: u32,
    #[cfg(feature = "debug")]
    #[new(default)]
    #[hidden_element]
    pub steps_vertex_buffer: Option<Arc<Buffer>>,
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum EntityType {
    Warp,
    Hidden,
    Player,
    Npc,
    Monster,
}
impl fmt::Display for EntityType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            EntityType::Warp => write!(f, "{:?}", "Warp"),
            EntityType::Hidden => write!(f, "{:?}", "Hidden"),
            EntityType::Player => write!(f, "{:?}", "Player"),
            EntityType::Npc => write!(f, "{:?}", "Npc"),
            EntityType::Monster => write!(f, "{:?}", "Monster"),
        } 
    }
}



#[derive(PrototypeElement)]
pub struct Common {
    pub entity_id: EntityId,
    pub job_id: usize,
    pub head_id: usize,
    pub health_points: usize,
    pub maximum_health_points: usize,
    pub movement_speed: usize,
    pub head_direction: usize,
    pub sex: Sex,

    #[hidden_element]
    pub entity_type: EntityType,
    pub active_movement: Option<Movement>,
    pub animation_data: AnimationData,
    pub grid_position: Vector2<usize>,
    pub position: Vector3<f32>,
    #[hidden_element]
    details: ResourceState<String>,
    #[hidden_element]
    animation_state: AnimationState,
}

#[cfg_attr(feature = "debug", korangar_debug::profile)]
#[allow(clippy::invisible_characters)]
fn get_sprite_path_for_player_job(job_id: usize) -> &'static str {
    match job_id {
        0 => "ÃÊº¸ÀÚ",             // NOVICE
        1 => "°Ë»Ç",               // SWORDMAN
        2 => "À§Àúµå",             // MAGICIAN
        3 => "±Ã¼Ö",               // ARCHER
        4 => "¼ºÁ÷ÀÚ",             // ACOLYTE
        5 => "»ÓÀÎ",               // MERCHANT
        6 => "ΜΜΜÏ",               // THIEF
        7 => "±â»ç",               // KNIGHT
        8 => "¼ºÅõ»ç",             // PRIEST
        9 => "¸¶¹Ý»Ç",             // WIZARD
        10 => "Á¦Ã¶°ø",            // BLACKSMITH
        11 => "ÇåÅÍ",              // HUNTER
        12 => "¾î¼¼½Å",            // ASSASSIN
        13 => "¿£´ë¿î",            // CHICKEN
        14 => "Å©·ç¼¼ÀÌ´õ",        // CRUSADER
        15 => "¸ùÅ©",              // MONK
        16 => "¼¼ÀÌÁö",            // SAGE
        17 => "·Î±×",              // ROGUE
        18 => "¿¬±Ý¼ú»ç",          // ALCHEMIST
        19 => "¹Ùµå",              // BARD
        20 => "¹«Èñ",              // DANCER
        23 => "½´ÆÛ³ëºñ½º",        // SUPERNOVICE
        24 => "°Ç³Ê",              // GUNSLINGER
        25 => "´ÑÀÚ",              // NINJA
        4001 => "ÃÊº¸ÀÚ",          // NOVICE_H
        4002 => "°Ë»Ç",            // SWORDMAN_H
        4003 => "À§Àúµå",          // MAGICIAN_H
        4004 => "±Ã¼Ö",            // ARCHER_H
        4005 => "¼ºÁ÷ÀÚ",          // ACOLYTE_H
        4006 => "»ÓÀÎ",            // MERCHANT_H
        4007 => "ΜΜΜÏ",            // THIEF_H
        4008 => "·Îµå³ªÀÌÆ®",      // KNIGHT_H
        4009 => "ÇÏÀÌÇÁ¸®",        // PRIEST_H
        4010 => "ÇÏÀÌÀ§Àúµå",      // WIZARD_H
        4011 => "È­ÀÌÆ®½º¹Ì½º",     // BLACKSMITH_H
        4012 => "½º³ªÀÌÆÛ",        // HUNTER_H
        4013 => "¾î½Ø½ÅÅ©·Î½º",    // ASSASSIN_H
        4014 => "¿£´ë¿î",          // CHICKEN_H
        4015 => "Å©·ç¼¼ÀÌ´õ",      // CRUSADER_H
        4016 => "¸ùÅ©",            // MONK_H
        4017 => "¼¼ÀÌÁö",          // SAGE_H
        4018 => "·Î±×",            // ROGUE_H
        4019 => "¿¬±Ý¼ú»ç",        // ALCHEMIST_H
        4020 => "¹Ùµå",            // BARD_H
        4021 => "¹«Èñ",            // DANCER_H
        4023 => "½´ÆÛ³ëºñ½º",      // NOVICE_B
        4024 => "°Ë»Ç",            // SWORDMAN_B
        4025 => "À§Àúµå",          // MAGICIAN_B
        4026 => "±Ã¼Ö",            // ARCHER_B
        4027 => "¼ºÁ÷ÀÚ",          // ACOLYTE_B
        4028 => "»ÓÀÎ",            // MERCHANT_B
        4029 => "µµµÏ",            // THIEF_B
        4030 => "±â»ç",            // KNIGHT_B
        4031 => "¼ºÅõ»ç",          // PRIEST_B
        4032 => "¸¶¹Ý»Ç",          // WIZARD_B
        4033 => "Á¦Ã¶°ø",          // BLACKSMITH_B
        4034 => "ÇåÅÍ",            // HUNTER_B
        4035 => "¾î¼¼½Å",          // ASSASSIN_B
        4037 => "Å©·ç¼¼ÀÌ´õ",      // CRUSADER_B
        4038 => "¸ùÅ©",            // MONK_B
        4039 => "¼¼ÀÌÁö",          // SAGE_B
        4040 => "·Î±×",            // ROGUE_B
        4041 => "¿¬±Ý¼ú»ç",        // ALCHEMIST_B
        4042 => "¹Ùµå",            // BARD_B
        4043 => "¹«Èñ",            // DANCER_B
        4045 => "½´ÆÛ³ëºñ½º",      // SUPERNOVICE_B
        4054 => "·é³ªÀÌÆ®",        // RUNE_KNIGHT
        4055 => "¿ö·Ï",            // WARLOCK
        4056 => "·¹ÀÎÁ®",          // RANGER
        4057 => "¾ÆÅ©ºñ¼ó",        // ARCH_BISHOP
        4058 => "¹ÌÄÉ´Ð",          // MECHANIC
        4059 => "±æ·ÎÆ¾Å©·Î½º",    // GUILLOTINE_CROSS
        4066 => "°¡ΜÅ",            // ROYAL_GUARD
        4067 => "¼Ò¼­·¯",           // SORCERER
        4068 => "¹Î½ºÆ®·²",        // MINSTREL
        4069 => "¿ø´õ·¯",          // WANDERER
        4070 => "½´¶ó",            // SURA
        4071 => "Á¦³×¸¯",          // GENETIC
        4072 => "½¦µµ¿ìÃ¼ÀÌ¼­",     // SHADOW_CHASER
        4060 => "·é³ªÀÌÆ®",        // RUNE_KNIGHT_H
        4061 => "¿ö·Ï",            // WARLOCK_H
        4062 => "·¹ÀÎÁ®",          // RANGER_H
        4063 => "¾ÆÅ©ºñ¼ó",        // ARCH_BISHOP_H
        4064 => "¹ÌÄÉ´Ð",          // MECHANIC_H
        4065 => "±æ·ÎÆ¾Å©·Î½º",    // GUILLOTINE_CROSS_H
        4073 => "°¡ΜÅ",            // ROYAL_GUARD_H
        4074 => "¼Ò¼­·¯",           // SORCERER_H
        4075 => "¹Î½ºÆ®·²",        // MINSTREL_H
        4076 => "¿ø´õ·¯",          // WANDERER_H
        4077 => "½´¶ó",            // SURA_H
        4078 => "Á¦³×¸¯",          // GENETIC_H
        4079 => "½¦µµ¿ìÃ¼ÀÌ¼­",     // SHADOW_CHASER_H
        4096 => "·é³ªÀÌÆ®",        // RUNE_KNIGHT_B
        4097 => "¿ö·Ï",            // WARLOCK_B
        4098 => "·¹ÀÎÁ®",          // RANGER_B
        4099 => "¾ÆÅ©ºñ¼ó",        // ARCHBISHOP_B
        4100 => "¹ÌÄÉ´Ð",          // MECHANIC_B
        4101 => "±æ·ÎÆ¾Å©·Î½º",    // GUILLOTINE_CROSS_B
        4102 => "°¡µå",            // ROYAL_GUARD_B
        4103 => "¼Ò¼­·¯",           // SORCERER_B
        4104 => "¹Î½ºÆ®·²",        // MINSTREL_B
        4105 => "¿ø´õ·¯",          // WANDERER_B
        4106 => "½´¶ó",            // SURA_B
        4107 => "Á¦³×¸¯",          // GENETIC_B
        4108 => "½¦µµ¿ìÃ¼ÀÌ¼­",     // SHADOW_CHASER_B
        4046 => "ÅÂ±Ç¼Ò³â",        // TAEKWON
        4047 => "±Ç¼º",            // STAR
        4049 => "¼Ò¿ï¸µÄ¿",        // LINKER
        4190 => "½´ÆÛ³ëºñ½º",      // SUPERNOVICE2
        4211 => "KAGEROU",         // KAGEROU
        4212 => "OBORO",           // OBORO
        4215 => "REBELLION",       // REBELLION
        4222 => "´ÑÀÚ",            // NINJA_B
        4223 => "KAGEROU",         // KAGEROU_B
        4224 => "OBORO",           // OBORO_B
        4225 => "ÅÂ±Ç¼Ò³â",        // TAEKWON_B
        4226 => "±Ç¼º",            // STAR_B
        4227 => "¼Ò¿ï¸µÄ¿",        // LINKER_B
        4228 => "°Ç³Ê",            // GUNSLINGER_B
        4229 => "REBELLION",       // REBELLION_B
        4239 => "¼ºÁ¦",            // STAR EMPEROR
        4240 => "¼Ò¿ï¸®ÆÛ",        // SOUL REAPER
        4241 => "¼ºÁ¦",            // STAR_EMPEROR_B
        4242 => "¼Ò¿ï¸®ÆÛ",        // SOUL_REAPER_B
        4252 => "DRAGON_KNIGHT",   // DRAGON KNIGHT
        4253 => "MEISTER",         // MEISTER
        4254 => "SHADOW_CROSS",    // SHADOW CROSS
        4255 => "ARCH_MAGE",       // ARCH MAGE
        4256 => "CARDINAL",        // CARDINAL
        4257 => "WINDHAWK",        // WINDHAWK
        4258 => "IMPERIAL_GUARD",  // IMPERIAL GUARD
        4259 => "BIOLO",           // BIOLO
        4260 => "ABYSS_CHASER",    // ABYSS CHASER
        4261 => "ELEMETAL_MASTER", // ELEMENTAL MASTER
        4262 => "INQUISITOR",      // INQUISITOR
        4263 => "TROUBADOUR",      // TROUBADOUR
        4264 => "TROUVERE",        // TROUVERE
        4302 => "SKY_EMPEROR",     // SKY EMPEROR
        4303 => "SOUL_ASCETIC",    // SOUL ASCETIC
        4304 => "SHINKIRO",        // SHINKIRO
        4305 => "SHIRANUI",        // SHIRANUI
        4306 => "NIGHT_WATCH",     // NIGHT WATCH
        4307 => "HYPER_NOVICE",    // HYPER NOVICE
        _ => "ÃÊº¸ÀÚ",             // NOVICE
    }
}

// This part instead of generating the sprite and actions, you need to generate
// the animation_loader.

fn get_entity_filename(script_loader: &ScriptLoader, entity_type: EntityType, job_id: usize, head_id: usize, sex: Sex) -> Vec<String> {
    let sex_sprite_path = match sex == Sex::Female {
        true => "¿©",
        false => "³²",
    };
    fn player_body_path(sex_sprite_path: &str, job_id: usize) -> String {
        format!(
            "ÀÎ°£Á·\\¸öÅë\\{}\\{}_{}",
            sex_sprite_path,
            get_sprite_path_for_player_job(job_id),
            sex_sprite_path
        )
    }
    fn player_head_path(sex_sprite_path: &str, head_id: usize) -> String {
        format!("ÀÎ°£Á·\\¸Ó¸®Åë\\{}\\{}_{}", sex_sprite_path, head_id, sex_sprite_path)
    }
    let entity_filename = match entity_type {
        EntityType::Player => vec![player_body_path(sex_sprite_path, job_id), player_head_path(sex_sprite_path, head_id+1)],
        EntityType::Npc => vec![format!("npc\\{}", script_loader.get_job_name_from_id(job_id))],
        EntityType::Monster => vec![format!("¸ó½ºÅÍ\\{}", script_loader.get_job_name_from_id(job_id))],
        EntityType::Warp | EntityType::Hidden => vec![format!("npc\\{}", script_loader.get_job_name_from_id(job_id))]
    };

    entity_filename
}

impl Common {
    pub fn new(
        sprite_loader: &mut SpriteLoader,
        action_loader: &mut ActionLoader,
        animation_loader: &mut AnimationLoader,
        script_loader: &ScriptLoader,
        map: &Map,
        entity_data: EntityData,
        client_tick: ClientTick,
    ) -> Self {
        let entity_id = entity_data.entity_id;
        let job_id = entity_data.job as usize;
        let head_id = entity_data.head as usize;
        let grid_position = entity_data.position;
        let grid_position = Vector2::new(grid_position.x, grid_position.y);
        let position = map.get_world_position(grid_position);
        let head_direction = entity_data.head_direction;

        let movement_speed = entity_data.movement_speed as usize;
        let health_points = entity_data.health_points as usize;
        let maximum_health_points = entity_data.maximum_health_points as usize;
        let sex = entity_data.sex;

        let active_movement = None;

        let entity_type = match job_id {
            45 => EntityType::Warp,
            111 => EntityType::Hidden, // TODO: check that this is correct
            // 111 | 139 => None,
            0..=44 | 4000..=5999 => EntityType::Player,
            46..=999 => EntityType::Npc,
            1000..=3999 => EntityType::Monster,
            _ => EntityType::Npc,
        };

        let entity_filename: Vec<String> = get_entity_filename(script_loader, entity_type, job_id, head_id, sex);
        // generate animation
        let animation_data = animation_loader.get_animation_data(sprite_loader, action_loader, entity_filename, entity_type);

        let details = ResourceState::Unavailable;
        let animation_state = AnimationState::new(client_tick);

        let mut common = Self {
            grid_position,
            position,
            entity_id,
            job_id,
            head_id,
            head_direction,
            sex,
            active_movement,
            entity_type,
            movement_speed,
            health_points,
            maximum_health_points,
            animation_data,
            details,
            animation_state,
        };

        if let Some(destination) = entity_data.destination {
            let position_from = Vector2::new(entity_data.position.x, entity_data.position.y);
            let position_to = Vector2::new(destination.x, destination.y);
            common.move_from_to(map, position_from, position_to, client_tick);
        }

        common
    }

    pub fn reload_sprite(
        &mut self,
        sprite_loader: &mut SpriteLoader,
        action_loader: &mut ActionLoader,
        animation_loader: &mut AnimationLoader,
        script_loader: &ScriptLoader,
    ) {
        let entity_filename: Vec<String> = get_entity_filename(script_loader, self.entity_type, self.job_id, self.head_id, self.sex);
        self.animation_data = animation_loader.get_animation_data(sprite_loader, action_loader, entity_filename, self.entity_type);
    }

    pub fn set_position(&mut self, map: &Map, position: Vector2<usize>, client_tick: ClientTick) {
        self.grid_position = position;
        self.position = map.get_world_position(position);
        self.active_movement = None;
        self.animation_state.idle(client_tick);
    }

    pub fn update(&mut self, map: &Map, _delta_time: f32, client_tick: ClientTick) {
        if let Some(active_movement) = self.active_movement.take() {
            let last_step = active_movement.steps.last().unwrap();

            if client_tick.0 > last_step.1 {
                let position = Vector2::new(last_step.0.x, last_step.0.y);
                self.set_position(map, position, client_tick);
            } else {
                let mut last_step_index = 0;
                while active_movement.steps[last_step_index + 1].1 < client_tick.0 {
                    last_step_index += 1;
                }

                let last_step = active_movement.steps[last_step_index];
                let next_step = active_movement.steps[last_step_index + 1];

                let array = (last_step.0 - next_step.0).map(|c| c as isize);
                let array: &[isize; 2] = array.as_ref();
                self.head_direction = match array {
                    [0, 1] => 0,
                    [1, 1] => 1,
                    [1, 0] => 2,
                    [1, -1] => 3,
                    [0, -1] => 4,
                    [-1, -1] => 5,
                    [-1, 0] => 6,
                    [-1, 1] => 7,
                    _ => panic!("impossible step"),
                };

                let last_step_position = map.get_world_position(last_step.0);
                let next_step_position = map.get_world_position(next_step.0);

                let clamped_tick = u32::max(last_step.1, client_tick.0);
                let total = next_step.1 - last_step.1;
                let offset = clamped_tick - last_step.1;

                let movement_elapsed = (1.0 / total as f32) * offset as f32;
                let position = last_step_position.lerp(next_step_position, movement_elapsed);

                self.position = position;
                self.active_movement = active_movement.into();
            }
        }

        self.animation_state.update(client_tick);
    }

    pub fn move_from_to(&mut self, map: &Map, from: Vector2<usize>, to: Vector2<usize>, starting_timestamp: ClientTick) {
        use pathfinding::prelude::astar;

        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        struct Pos(usize, usize);

        impl Pos {
            fn successors(&self, map: &Map) -> Vec<Pos> {
                let &Pos(x, y) = self;
                let mut successors = Vec::new();

                if map.x_in_bounds(x + 1) {
                    successors.push(Pos(x + 1, y));
                }

                if x > 0 {
                    successors.push(Pos(x - 1, y));
                }

                if map.y_in_bounds(y + 1) {
                    successors.push(Pos(x, y + 1));
                }

                if y > 0 {
                    successors.push(Pos(x, y - 1));
                }

                if map.x_in_bounds(x + 1)
                    && map.y_in_bounds(y + 1)
                    && map.get_tile(Vector2::new(x + 1, y)).flags.contains(TileFlags::WALKABLE)
                    && map.get_tile(Vector2::new(x, y + 1)).flags.contains(TileFlags::WALKABLE)
                {
                    successors.push(Pos(x + 1, y + 1));
                }

                if x > 0
                    && map.y_in_bounds(y + 1)
                    && map.get_tile(Vector2::new(x - 1, y)).flags.contains(TileFlags::WALKABLE)
                    && map.get_tile(Vector2::new(x, y + 1)).flags.contains(TileFlags::WALKABLE)
                {
                    successors.push(Pos(x - 1, y + 1));
                }

                if map.x_in_bounds(x + 1)
                    && y > 0
                    && map.get_tile(Vector2::new(x + 1, y)).flags.contains(TileFlags::WALKABLE)
                    && map.get_tile(Vector2::new(x, y - 1)).flags.contains(TileFlags::WALKABLE)
                {
                    successors.push(Pos(x + 1, y - 1));
                }

                if x > 0
                    && y > 0
                    && map.get_tile(Vector2::new(x - 1, y)).flags.contains(TileFlags::WALKABLE)
                    && map.get_tile(Vector2::new(x, y - 1)).flags.contains(TileFlags::WALKABLE)
                {
                    successors.push(Pos(x - 1, y - 1));
                }

                let successors = successors
                    .drain(..)
                    .filter(|Pos(x, y)| map.get_tile(Vector2::new(*x, *y)).flags.contains(TileFlags::WALKABLE))
                    .collect::<Vec<Pos>>();

                successors
            }

            fn convert_to_vector(self) -> Vector2<usize> {
                Vector2::new(self.0, self.1)
            }
        }

        let result = astar(
            &Pos(from.x, from.y),
            |position| position.successors(map).into_iter().map(|position| (position, 0)),
            |position| -> usize {
                // Values taken from rAthena.
                const MOVE_COST: usize = 10;
                const DIAGONAL_MOVE_COST: usize = 14;

                let distance_x = usize::abs_diff(position.0, to.x);
                let distance_y = usize::abs_diff(position.1, to.y);

                let straight_moves = usize::abs_diff(distance_x, distance_y);
                let diagonal_moves = usize::min(distance_x, distance_y);

                DIAGONAL_MOVE_COST * diagonal_moves + MOVE_COST * straight_moves
            },
            |position| *position == Pos(to.x, to.y),
        )
        .map(|x| x.0);

        if let Some(path) = result {
            let mut last_timestamp = starting_timestamp.0;
            let mut last_position: Option<Vector2<usize>> = None;

            let steps: Vec<(Vector2<usize>, u32)> = path
                .into_iter()
                .map(|pos| {
                    if let Some(position) = last_position {
                        const DIAGONAL_MULTIPLIER: f32 = 1.4;

                        let speed = match position.x == pos.0 || position.y == pos.1 {
                            // true means we are moving orthogonally
                            true => self.movement_speed as u32,
                            // false means we are moving diagonally
                            false => (self.movement_speed as f32 * DIAGONAL_MULTIPLIER) as u32,
                        };

                        let arrival_position = pos.convert_to_vector();
                        let arrival_timestamp = last_timestamp + speed;

                        last_timestamp = arrival_timestamp;
                        last_position = Some(arrival_position);

                        (arrival_position, arrival_timestamp)
                    } else {
                        last_position = Some(from);
                        (from, last_timestamp)
                    }
                })
                .collect();

            // If there is only a single step the player is already on the correct tile.
            if steps.len() > 1 {
                self.active_movement = Movement::new(steps, starting_timestamp.0).into();

                if self.animation_state.action != 1 {
                    self.animation_state.walk(self.movement_speed, starting_timestamp);
                }
            }
        }
    }

    /*#[cfg(feature = "debug")]
    fn generate_step_texture_coordinates(
        steps: &Vec<(Vector2<usize>, u32)>,
        step: Vector2<usize>,
        index: usize,
    ) -> ([Vector2<f32>; 4], i32) {
        if steps.len() - 1 == index {
            return (
                [
                    Vector2::new(0.0, 1.0),
                    Vector2::new(1.0, 1.0),
                    Vector2::new(1.0, 0.0),
                    Vector2::new(0.0, 0.0),
                ],
                0,
            );
        }

        let delta = steps[index + 1].0.map(|component| component as isize) - step.map(|component| component as isize);

        match delta {
            Vector2 { x: 1, y: 0 } => (
                [
                    Vector2::new(0.0, 0.0),
                    Vector2::new(1.0, 0.0),
                    Vector2::new(1.0, 1.0),
                    Vector2::new(0.0, 1.0),
                ],
                1,
            ),
            Vector2 { x: -1, y: 0 } => (
                [
                    Vector2::new(1.0, 0.0),
                    Vector2::new(0.0, 0.0),
                    Vector2::new(0.0, 1.0),
                    Vector2::new(1.0, 1.0),
                ],
                1,
            ),
            Vector2 { x: 0, y: 1 } => (
                [
                    Vector2::new(0.0, 0.0),
                    Vector2::new(0.0, 1.0),
                    Vector2::new(1.0, 1.0),
                    Vector2::new(1.0, 0.0),
                ],
                1,
            ),
            Vector2 { x: 0, y: -1 } => (
                [
                    Vector2::new(1.0, 0.0),
                    Vector2::new(1.0, 1.0),
                    Vector2::new(0.0, 1.0),
                    Vector2::new(0.0, 0.0),
                ],
                1,
            ),
            Vector2 { x: 1, y: 1 } => (
                [
                    Vector2::new(0.0, 1.0),
                    Vector2::new(0.0, 0.0),
                    Vector2::new(1.0, 0.0),
                    Vector2::new(1.0, 1.0),
                ],
                2,
            ),
            Vector2 { x: -1, y: 1 } => (
                [
                    Vector2::new(0.0, 0.0),
                    Vector2::new(0.0, 1.0),
                    Vector2::new(1.0, 1.0),
                    Vector2::new(1.0, 0.0),
                ],
                2,
            ),
            Vector2 { x: 1, y: -1 } => (
                [
                    Vector2::new(1.0, 1.0),
                    Vector2::new(1.0, 0.0),
                    Vector2::new(0.0, 0.0),
                    Vector2::new(0.0, 1.0),
                ],
                2,
            ),
            Vector2 { x: -1, y: -1 } => (
                [
                    Vector2::new(1.0, 0.0),
                    Vector2::new(1.0, 1.0),
                    Vector2::new(0.0, 1.0),
                    Vector2::new(0.0, 0.0),
                ],
                2,
            ),
            _other => panic!("incorrent pathing"),
        }
    }

    #[cfg(feature = "debug")]
    pub fn generate_steps_vertex_buffer(&mut self, device: Arc<Device>, map: &Map) {
        let mut native_steps_vertices = Vec::new();
        let mut active_movement = self.active_movement.as_mut().unwrap();

        for (index, (step, _)) in active_movement.steps.iter().cloned().enumerate() {
            let tile = map.get_tile(step);
            let offset = Vector2::new(step.x as f32 * 5.0, step.y as f32 * 5.0);

            let first_position = Vector3::new(offset.x, tile.upper_left_height + 1.0, offset.y);
            let second_position = Vector3::new(offset.x + 5.0, tile.upper_right_height + 1.0, offset.y);
            let third_position = Vector3::new(offset.x + 5.0, tile.lower_right_height + 1.0, offset.y + 5.0);
            let fourth_position = Vector3::new(offset.x, tile.lower_left_height + 1.0, offset.y + 5.0);

            let first_normal = NativeModelVertex::calculate_normal(first_position, second_position, third_position);
            let second_normal = NativeModelVertex::calculate_normal(fourth_position, first_position, third_position);

            let (texture_coordinates, texture_index) = Self::generate_step_texture_coordinates(&active_movement.steps, step, index);

            native_steps_vertices.push(NativeModelVertex::new(
                first_position,
                first_normal,
                texture_coordinates[0],
                texture_index,
                0.0,
            ));
            native_steps_vertices.push(NativeModelVertex::new(
                second_position,
                first_normal,
                texture_coordinates[1],
                texture_index,
                0.0,
            ));
            native_steps_vertices.push(NativeModelVertex::new(
                third_position,
                first_normal,
                texture_coordinates[2],
                texture_index,
                0.0,
            ));

            native_steps_vertices.push(NativeModelVertex::new(
                first_position,
                second_normal,
                texture_coordinates[0],
                texture_index,
                0.0,
            ));
            native_steps_vertices.push(NativeModelVertex::new(
                third_position,
                second_normal,
                texture_coordinates[2],
                texture_index,
                0.0,
            ));
            native_steps_vertices.push(NativeModelVertex::new(
                fourth_position,
                second_normal,
                texture_coordinates[3],
                texture_index,
                0.0,
            ));
        }

        let vertex_buffer_usage = BufferUsage {
            vertex_buffer: true,
            ..Default::default()
        };

        let steps_vertices = NativeModelVertex::to_vertices(native_steps_vertices);
        let vertex_buffer = CpuAccessibleBuffer::from_iter(
            self.memory_allocator,
            BufferUsage {
                vertex_buffer: true,
                ..Default::default()
            },
            false,
            steps_vertices.into_iter(),
        )
        .unwrap();
        active_movement.steps_vertex_buffer = Some(vertex_buffer);
    }*/

    pub fn render<T>(&self, render_target: &mut T::Target, render_pass: &mut RenderPass, renderer: &T, camera: &dyn Camera)
    where
        T: Renderer + EntityRenderer,
    {
        /*if self.animation_data.animation_pair.len() == 1 {
            // TODO: Made everything using the animation, deprecate the texture from
            // SpriteLoader.
            for constructor in self.animation_data.animation_pair.iter() {
                let camera_direction = camera.camera_direction();
                let (texture, position, mirror) = constructor.actions.render(
                    &constructor.sprites,
                    &self.animation_state,
                    camera_direction,
                    self.head_direction,
                );

                renderer.render_entity(
                    render_target,
                    render_pass,
                    camera,
                    texture,
                    self.position,
                    Vector3::new(0.0, 0.0, 0.0),
                    Vector2::from_value(0.7),
                    Vector2::new(1, 1),
                    Vector2::new(0, 0),
                    mirror,
                    self.entity_id,
                );
            }
        } else {*/
            let camera_direction = camera.camera_direction();
            let (texture, mirror) = self
                .animation_data
                .render(&self.animation_state, camera_direction, self.head_direction);
            renderer.render_entity(
                render_target,
                render_pass,
                camera,
                texture,
                self.position,
                Vector3::new(0.0, 0.0, 0.0),
                Vector2::from_value(0.7),
                Vector2::new(1, 1),
                Vector2::new(0, 0),
                mirror,
                self.entity_id,
            );
        /* }*/
    }

    #[cfg(feature = "debug")]
    pub fn render_marker<T>(
        &self,
        render_target: &mut T::Target,
        render_pass: &mut RenderPass,
        renderer: &T,
        camera: &dyn Camera,
        marker_identifier: MarkerIdentifier,
        hovered: bool,
    ) where
        T: Renderer + MarkerRenderer,
    {
        renderer.render_marker(render_target, render_pass, camera, marker_identifier, self.position, hovered);
    }
}

#[derive(PrototypeWindow)]
pub struct Player {
    common: Common,
    pub spell_points: usize,
    pub activity_points: usize,
    pub maximum_spell_points: usize,
    pub maximum_activity_points: usize,
}

impl Player {
    pub fn new(
        sprite_loader: &mut SpriteLoader,
        action_loader: &mut ActionLoader,
        animation_loader: &mut AnimationLoader,
        script_loader: &ScriptLoader,
        map: &Map,
        account_id: AccountId,
        character_information: CharacterInformation,
        player_position: WorldPosition,
        client_tick: ClientTick,
    ) -> Self {
        let spell_points = character_information.spell_points as usize;
        let activity_points = 0;
        let maximum_spell_points = character_information.maximum_spell_points as usize;
        let maximum_activity_points = 0;
        let common = Common::new(
            sprite_loader,
            action_loader,
            animation_loader,
            script_loader,
            map,
            EntityData::from_character(account_id, character_information, player_position),
            client_tick,
        );

        Self {
            common,
            spell_points,
            activity_points,
            maximum_spell_points,
            maximum_activity_points,
        }
    }

    pub fn get_common(&self) -> &Common {
        &self.common
    }

    pub fn get_common_mut(&mut self) -> &mut Common {
        &mut self.common
    }

    pub fn update_status(&mut self, status_type: StatusType) {
        match status_type {
            StatusType::MaximumHealthPoints(value) => self.common.maximum_health_points = value as usize,
            StatusType::MaximumSpellPoints(value) => self.maximum_spell_points = value as usize,
            StatusType::HealthPoints(value) => self.common.health_points = value as usize,
            StatusType::SpellPoints(value) => self.spell_points = value as usize,
            StatusType::ActivityPoints(value) => self.activity_points = value as usize,
            StatusType::MaximumActivityPoints(value) => self.maximum_activity_points = value as usize,
            _ => {}
        }
    }

    pub fn render_status(
        &self,
        render_target: &mut <DeferredRenderer as Renderer>::Target,
        render_pass: &mut RenderPass,
        renderer: &DeferredRenderer,
        camera: &dyn Camera,
        theme: &GameTheme,
        window_size: ScreenSize,
    ) {
        let (view_matrix, projection_matrix) = camera.view_projection_matrices();
        let clip_space_position = (projection_matrix * view_matrix) * self.common.position.extend(1.0);
        let screen_position = camera.clip_to_screen_space(clip_space_position);
        let final_position = ScreenPosition {
            left: screen_position.x * window_size.width,
            top: screen_position.y * window_size.height + 5.0,
        };

        let bar_width = theme.status_bar.player_bar_width.get();
        let gap = theme.status_bar.gap.get();
        let total_height = theme.status_bar.health_height.get()
            + theme.status_bar.spell_point_height.get()
            + theme.status_bar.activity_point_height.get()
            + gap * 2.0;

        let mut offset = 0.0;

        let background_position = final_position - theme.status_bar.border_size.get() - ScreenSize::only_width(bar_width / 2.0);

        let background_size = ScreenSize {
            width: bar_width,
            height: total_height,
        } + theme.status_bar.border_size.get() * 2.0;

        renderer.render_rectangle(
            render_target,
            render_pass,
            background_position,
            background_size,
            theme.status_bar.background_color.get(),
        );

        renderer.render_bar(
            render_target,
            render_pass,
            final_position,
            ScreenSize {
                width: bar_width,
                height: theme.status_bar.health_height.get(),
            },
            theme.status_bar.player_health_color.get(),
            self.common.maximum_health_points as f32,
            self.common.health_points as f32,
        );

        offset += gap + theme.status_bar.health_height.get();

        renderer.render_bar(
            render_target,
            render_pass,
            final_position + ScreenPosition::only_top(offset),
            ScreenSize {
                width: bar_width,
                height: theme.status_bar.spell_point_height.get(),
            },
            theme.status_bar.spell_point_color.get(),
            self.maximum_spell_points as f32,
            self.spell_points as f32,
        );

        offset += gap + theme.status_bar.spell_point_height.get();

        renderer.render_bar(
            render_target,
            render_pass,
            final_position + ScreenPosition::only_top(offset),
            ScreenSize {
                width: bar_width,
                height: theme.status_bar.activity_point_height.get(),
            },
            theme.status_bar.activity_point_color.get(),
            self.maximum_activity_points as f32,
            self.activity_points as f32,
        );
    }
}

#[derive(PrototypeWindow)]
pub struct Npc {
    common: Common,
}

impl Npc {
    pub fn new(
        sprite_loader: &mut SpriteLoader,
        action_loader: &mut ActionLoader,
        animation_loader: &mut AnimationLoader,
        script_loader: &ScriptLoader,
        map: &Map,
        entity_data: EntityData,
        client_tick: ClientTick,
    ) -> Self {
        let common = Common::new(
            sprite_loader,
            action_loader,
            animation_loader,
            script_loader,
            map,
            entity_data,
            client_tick,
        );

        Self { common }
    }

    pub fn get_common(&self) -> &Common {
        &self.common
    }

    pub fn get_common_mut(&mut self) -> &mut Common {
        &mut self.common
    }

    pub fn render_status(
        &self,
        render_target: &mut <DeferredRenderer as Renderer>::Target,
        render_pass: &mut RenderPass,
        renderer: &DeferredRenderer,
        camera: &dyn Camera,
        theme: &GameTheme,
        window_size: ScreenSize,
    ) {
        if self.common.entity_type != EntityType::Monster {
            return;
        }

        let (view_matrix, projection_matrix) = camera.view_projection_matrices();
        let clip_space_position = (projection_matrix * view_matrix) * self.common.position.extend(1.0);
        let screen_position = camera.clip_to_screen_space(clip_space_position);
        let final_position = ScreenPosition {
            left: screen_position.x * window_size.width,
            top: screen_position.y * window_size.height + 5.0,
        };

        let bar_width = theme.status_bar.enemy_bar_width.get();

        renderer.render_rectangle(
            render_target,
            render_pass,
            final_position - theme.status_bar.border_size.get() - ScreenSize::only_width(bar_width / 2.0),
            ScreenSize {
                width: bar_width,
                height: theme.status_bar.enemy_health_height.get(),
            } + (theme.status_bar.border_size.get() * 2.0),
            theme.status_bar.background_color.get(),
        );

        renderer.render_bar(
            render_target,
            render_pass,
            final_position,
            ScreenSize {
                width: bar_width,
                height: theme.status_bar.enemy_health_height.get(),
            },
            theme.status_bar.enemy_health_color.get(),
            self.common.maximum_health_points as f32,
            self.common.health_points as f32,
        );
    }
}

// TODO:
//#[derive(PrototypeWindow)]
pub enum Entity {
    Player(Player),
    Npc(Npc),
}

impl Entity {
    fn get_common(&self) -> &Common {
        match self {
            Self::Player(player) => player.get_common(),
            Self::Npc(npc) => npc.get_common(),
        }
    }

    fn get_common_mut(&mut self) -> &mut Common {
        match self {
            Self::Player(player) => player.get_common_mut(),
            Self::Npc(npc) => npc.get_common_mut(),
        }
    }

    pub fn get_entity_id(&self) -> EntityId {
        self.get_common().entity_id
    }

    pub fn get_entity_type(&self) -> EntityType {
        self.get_common().entity_type
    }

    pub fn are_details_unavailable(&self) -> bool {
        match &self.get_common().details {
            ResourceState::Unavailable => true,
            _requested_or_available => false,
        }
    }

    pub fn set_job(&mut self, job_id: usize) {
        self.get_common_mut().job_id = job_id;
    }

    pub fn set_head(&mut self, head_id: usize) {
        self.get_common_mut().head_id = head_id;
    }

    pub fn reload_sprite(
        &mut self,
        sprite_loader: &mut SpriteLoader,
        action_loader: &mut ActionLoader,
        animation_loader: &mut AnimationLoader,
        script_loader: &ScriptLoader,
    ) {
        self.get_common_mut()
            .reload_sprite(sprite_loader, action_loader, animation_loader, script_loader);
    }

    pub fn set_details_requested(&mut self) {
        self.get_common_mut().details = ResourceState::Requested;
    }

    pub fn set_details(&mut self, details: String) {
        self.get_common_mut().details = ResourceState::Available(details);
    }

    pub fn get_details(&self) -> Option<&String> {
        self.get_common().details.as_option()
    }

    pub fn get_grid_position(&self) -> Vector2<usize> {
        self.get_common().grid_position
    }

    pub fn get_position(&self) -> Vector3<f32> {
        self.get_common().position
    }

    pub fn set_position(&mut self, map: &Map, position: Vector2<usize>, client_tick: ClientTick) {
        self.get_common_mut().set_position(map, position, client_tick);
    }

    pub fn set_death(&mut self, client_tick: ClientTick) {
        self.get_common_mut().animation_state.death(client_tick);
    }

    pub fn set_idle(&mut self, client_tick: ClientTick) {
        self.get_common_mut().animation_state.idle(client_tick);
    }


    pub fn update_health(&mut self, health_points: usize, maximum_health_points: usize) {
        let common = self.get_common_mut();
        common.health_points = health_points;
        common.maximum_health_points = maximum_health_points;
    }

    pub fn update(&mut self, map: &Map, delta_time: f32, client_tick: ClientTick) {
        self.get_common_mut().update(map, delta_time, client_tick);
    }

    pub fn move_from_to(&mut self, map: &Map, from: Vector2<usize>, to: Vector2<usize>, starting_timestamp: ClientTick) {
        self.get_common_mut().move_from_to(map, from, to, starting_timestamp);
    }

    /*#[cfg(feature = "debug")]
    pub fn generate_steps_vertex_buffer(&mut self, device: Arc<Device>, map: &Map) {
        self.get_common_mut().generate_steps_vertex_buffer(device, map);
    }*/

    pub fn render<T>(&self, render_target: &mut T::Target, render_pass: &mut RenderPass, renderer: &T, camera: &dyn Camera)
    where
        T: Renderer + EntityRenderer,
    {
        self.get_common().render(render_target, render_pass, renderer, camera);
    }

    #[cfg(feature = "debug")]
    pub fn render_marker<T>(
        &self,
        render_target: &mut T::Target,
        render_pass: &mut RenderPass,
        renderer: &T,
        camera: &dyn Camera,
        marker_identifier: MarkerIdentifier,
        hovered: bool,
    ) where
        T: Renderer + MarkerRenderer,
    {
        self.get_common()
            .render_marker(render_target, render_pass, renderer, camera, marker_identifier, hovered);
    }

    pub fn render_status(
        &self,
        render_target: &mut <DeferredRenderer as Renderer>::Target,
        render_pass: &mut RenderPass,
        renderer: &DeferredRenderer,
        camera: &dyn Camera,
        theme: &GameTheme,
        window_size: ScreenSize,
    ) {
        match self {
            Self::Player(player) => player.render_status(render_target, render_pass, renderer, camera, theme, window_size),
            Self::Npc(npc) => npc.render_status(render_target, render_pass, renderer, camera, theme, window_size),
        }
    }
}

impl PrototypeWindow<InterfaceSettings> for Entity {
    fn to_window(
        &self,
        window_cache: &WindowCache,
        application: &InterfaceSettings,
        available_space: ScreenSize,
    ) -> Window<InterfaceSettings> {
        match self {
            Entity::Player(player) => player.to_window(window_cache, application, available_space),
            Entity::Npc(npc) => npc.to_window(window_cache, application, available_space),
        }
    }
}
