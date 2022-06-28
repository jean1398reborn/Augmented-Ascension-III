use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4};

use crate::assets::AssetType;
use crate::char::{AttackBuffer, CharEntities, Character};
use crate::collider::{
    DeathColliderIdentifier, JumpResetColliderIdentifier, SolidColliderIdentifier,
};
use crate::projectile::ProjectileIdentifier;
use crate::rigidbody::PhysicsSpawnExtras;
use crate::{
    AAPlayerDescriptor, AttackInstanceDirectory, CharComponentMap, ColliderSyncEntity,
    DirectionFacingFlags, Map, PlayerHealth, PlayerIdentifier, QueryEntityError,
};
use bevy::prelude::*;
use bevy_inspector_egui::Inspectable;
use bevy_rapier2d::prelude::*;
use bevy_rapier2d::rapier::geometry::CollisionEventFlags;
use bevy_svg::prelude::Svg;
use igd::aio::Gateway;
use igd::{AddPortError, PortMappingProtocol, SearchOptions};
use serde::{Deserialize, Serialize};
use std::io::Write;
use tokio::net::{TcpListener, UdpSocket};

pub struct Game {
    pub selected_characters: HashMap<u64, Character>,
    pub available_characters: Vec<Character>,
    pub selected_map: Map,
    pub available_maps: Vec<Map>,
}

pub enum CollisionEventType {
    Started,
    Stopped,
}

pub enum CollisionPlayerType {
    None,
    Two(FoundPlayerRigidId, (Entity, Entity)),
    Single(FoundPlayerRigidId, Entity),
}

pub enum FoundPlayerRigidId {
    ColliderOne,
    ColliderTwo,
    Both,
}

pub fn process_collision_event(
    player_identifier: &mut Query<(
        &mut PlayerIdentifier,
        &mut AAPlayerDescriptor,
        &mut PlayerHealth,
    )>,
    parent_query: &Query<&Parent>,
    collider_one: Entity,
    collider_two: Entity,
) -> CollisionPlayerType {
    // get the parent of the collider since the collider is a child of the actual player rigidbody
    let collider_one = match parent_query.get(collider_one) {
        Ok(parent) => parent.0,
        Err(_) => collider_one,
    };

    let collider_two = match parent_query.get(collider_two) {
        Ok(parent) => parent.0,
        Err(_) => collider_two,
    };

    let mut player_possibility_one = match player_identifier.contains(collider_one) {
        true => Some(collider_one),
        false => None,
    };

    let mut player_possibility_two = match player_identifier.contains(collider_two) {
        true => Some(collider_two),
        false => None,
    };

    match player_possibility_one {
        None => match player_possibility_two {
            None => {
                return CollisionPlayerType::None;
            }
            Some(player_id) => {
                return CollisionPlayerType::Single(FoundPlayerRigidId::ColliderTwo, player_id);
            }
        },
        Some(player_one) => match player_possibility_two {
            None => {
                return CollisionPlayerType::Single(FoundPlayerRigidId::ColliderOne, player_one);
            }
            Some(player_two) => {
                return CollisionPlayerType::Two(
                    FoundPlayerRigidId::Both,
                    (player_one, player_two),
                );
            }
        },
    }
}

pub fn single_update_player_death(
    player_body: Entity,
    potential_death: Entity,
    death_query: &Query<&DeathColliderIdentifier>,
    player_query: &mut Query<(
        &mut PlayerIdentifier,
        &mut AAPlayerDescriptor,
        &mut PlayerHealth,
    )>,
    collision_type: CollisionEventType,
) {
    // Check if its a jump reset collider else return skipping having to query for the player identifier
    match death_query.contains(potential_death) {
        true => {
            // Jump reset collider collision event confirmed
            // Check if it should reset or update the player jumps
            match collision_type {
                // Reset jumps when a collision has started with it
                CollisionEventType::Started => {
                    // Unwrapping is fine since we've filtered out player_body before this fn is run
                    let (_player_id, _player_desc, mut health) =
                        player_query.get_mut(player_body).unwrap();
                    health.current_health = 0.0;
                }
                CollisionEventType::Stopped => {
                    let (_player_id, _player_desc, mut health) =
                        player_query.get_mut(player_body).unwrap();
                    health.current_health = 0.0;
                }
            }
        }
        false => return,
    }
}

pub fn single_update_player_jump_reset(
    player_body: Entity,
    potential_jump_reset: Entity,
    jump_reset_query: &Query<&JumpResetColliderIdentifier>,
    player_query: &mut Query<(
        &mut PlayerIdentifier,
        &mut AAPlayerDescriptor,
        &mut PlayerHealth,
    )>,
    collision_type: CollisionEventType,
) {
    // Check if its a jump reset collider else return skipping having to query for the player identifier
    match jump_reset_query.contains(potential_jump_reset) {
        true => {
            // Jump reset collider collision event confirmed
            // Check if it should reset or update the player jumps
            match collision_type {
                // Reset jumps when a collision has started with it
                CollisionEventType::Started => {
                    // Unwrapping is fine since we've filtered out player_body before this fn is run
                    let (player_id, mut player_desc, _health) =
                        player_query.get_mut(player_body).unwrap();
                    player_desc.available_jumps = player_desc.maximum_jumps;
                    player_desc.char_collision_dominance = true;
                }
                CollisionEventType::Stopped => {
                    let (player_id, mut player_desc, _health) =
                        player_query.get_mut(player_body).unwrap();
                    player_desc.available_jumps = player_desc.maximum_jumps - 1;
                    player_desc.char_collision_dominance = false;
                }
            }
        }
        false => return,
    }
}

pub fn enforce_char_collision_dominance(mut dominance_query: Query<&mut AAPlayerDescriptor>) {
    for mut player in dominance_query.iter_mut() {
        if player.char_collision_dominance == true {
            player.available_jumps = player.maximum_jumps;
        }
    }
}

pub fn double_update_player_jump_reset(
    player_one: Entity,
    player_two: Entity,
    jump_reset_query: &Query<&JumpResetColliderIdentifier>,
    children_query: &Query<&Children>,
    player_query: &mut Query<(
        &mut PlayerIdentifier,
        &mut AAPlayerDescriptor,
        &mut PlayerHealth,
    )>,
    collision_type: CollisionEventType,
) {
    let players = [player_one, player_two];
    // check if any of the two players collided have the jump reset query as a collider under one of their children
    for player in players {
        match children_query.get(player) {
            Ok(children) => {
                for child in children.iter() {
                    match jump_reset_query.contains(*child) {
                        true => {
                            // Jump reset collider collision event confirmed
                            // Check if it should reset or update the player jumps
                            match collision_type {
                                // Reset jumps when a collision has started with it
                                CollisionEventType::Started => {
                                    // Unwrapping is fine since we've filtered out player_body before this fn is run
                                    let (player_id, mut player_desc, _health) =
                                        player_query.get_mut(player_one).unwrap();
                                    player_desc.available_jumps = player_desc.maximum_jumps;

                                    // Update it for player two too
                                    let (player_id, mut player_desc, _health) =
                                        player_query.get_mut(player_two).unwrap();
                                    player_desc.available_jumps = player_desc.maximum_jumps;
                                    return;
                                }
                                CollisionEventType::Stopped => {
                                    let (player_id, mut player_desc, _health) =
                                        player_query.get_mut(player_one).unwrap();

                                    // Check if jumps should be decreased
                                    if !player_desc.char_collision_dominance {
                                        player_desc.available_jumps = player_desc.maximum_jumps - 1;
                                    }

                                    let (player_id, mut player_desc, _health) =
                                        player_query.get_mut(player_two).unwrap();

                                    if !player_desc.char_collision_dominance {
                                        player_desc.available_jumps = player_desc.maximum_jumps - 1;
                                    }
                                    return;
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
}

pub fn projectile_hit_collision_event(
    char_entity: Entity,
    projectile_entity: Entity,
    projectile_query: &mut Query<(&mut ProjectileIdentifier)>,
    player_query: &mut Query<(
        &mut PlayerIdentifier,
        &mut AAPlayerDescriptor,
        &mut PlayerHealth,
    )>,
    parent_query: &Query<&Parent>,
    solid_collider_query: &Query<&SolidColliderIdentifier>,
) {
    let projectile_entity = match parent_query.get(projectile_entity) {
        Ok(parent) => parent.0,
        Err(_) => return,
    };

    match projectile_query.get_mut(projectile_entity) {
        Ok(mut projectile_id) => {
            let parent = match parent_query.get(char_entity) {
                Ok(parent) => parent,
                Err(_) => return,
            };

            if parent.0 == projectile_id.parent {
                return;
            };

            //check if its a solid collider since we dont want collision to happen with jump reset colliders:
            if !solid_collider_query.contains(char_entity) {
                return;
            }

            let (mut player_id, mut player_desc, mut health) = match player_query.get_mut(parent.0)
            {
                Ok(player) => player,
                Err(err) => {
                    warn!(
                        "Failed to find player for hit detection under id: {:#?} w/ err: {:#?}",
                        char_entity, err
                    );
                    return;
                }
            };

            projectile_id.apply_projectile(&mut health, &mut player_id, &mut player_desc);
        }
        Err(_projectile_err) => {}
    }
}

pub fn health_despawn_check(
    projectile_query: Query<(Entity, &PlayerHealth, &ColliderSyncEntity)>,
    mut commands: Commands,
) {
    for (entity, player_health, collider_entity) in projectile_query.iter() {
        if player_health.current_health <= 0.0 {
            collider_entity.despawn_self(&mut commands);
            commands.entity(entity).despawn_recursive();
        }
    }
}

pub fn collision_process(
    mut collision_events: EventReader<CollisionEvent>,
    jump_reset_query: Query<&JumpResetColliderIdentifier>,
    death_query: Query<&DeathColliderIdentifier>,
    solid_collider_query: Query<&SolidColliderIdentifier>,
    child_query: Query<&Children>,
    mut player_query: Query<(
        &mut PlayerIdentifier,
        &mut AAPlayerDescriptor,
        &mut PlayerHealth,
    )>,
    mut projectile_query: Query<(&mut ProjectileIdentifier)>,
    parent_query: Query<&Parent>,
) {
    for collision_event in collision_events.iter() {
        match collision_event {
            CollisionEvent::Started(collider_one, collider_two, event_flags) => {
                if event_flags.contains(CollisionEventFlags::REMOVED) {
                    continue;
                }

                let player_possibilities = process_collision_event(
                    &mut player_query,
                    &parent_query,
                    *collider_one,
                    *collider_two,
                );

                match player_possibilities {
                    CollisionPlayerType::None => {}
                    CollisionPlayerType::Single(found, player_id) => {
                        let (other_collider, original_collider) = match found {
                            FoundPlayerRigidId::ColliderOne => (*collider_two, *collider_one),
                            FoundPlayerRigidId::ColliderTwo => (*collider_one, *collider_two),
                            _ => {
                                panic!("Impossible Sequence Occurred In Collision Event")
                            }
                        };

                        single_update_player_jump_reset(
                            player_id,
                            other_collider,
                            &jump_reset_query,
                            &mut player_query,
                            CollisionEventType::Started,
                        );

                        single_update_player_death(
                            player_id,
                            other_collider,
                            &death_query,
                            &mut player_query,
                            CollisionEventType::Started,
                        );

                        projectile_hit_collision_event(
                            original_collider,
                            other_collider,
                            &mut projectile_query,
                            &mut player_query,
                            &parent_query,
                            &solid_collider_query,
                        );
                    }
                    CollisionPlayerType::Two(found, (player_one, player_two)) => {
                        double_update_player_jump_reset(
                            player_one,
                            player_two,
                            &jump_reset_query,
                            &child_query,
                            &mut player_query,
                            CollisionEventType::Started,
                        );
                    }
                }
            }
            CollisionEvent::Stopped(collider_one, collider_two, event_flags) => {
                if event_flags.contains(CollisionEventFlags::REMOVED) {
                    continue;
                }

                let player_possibilities = process_collision_event(
                    &mut player_query,
                    &parent_query,
                    *collider_one,
                    *collider_two,
                );

                match player_possibilities {
                    CollisionPlayerType::None => {}
                    CollisionPlayerType::Single(found, player_id) => {
                        let (other_collider, original_collider) = match found {
                            FoundPlayerRigidId::ColliderOne => (*collider_two, *collider_one),
                            FoundPlayerRigidId::ColliderTwo => (*collider_one, *collider_two),
                            _ => {
                                panic!("Impossible Sequence Occurred In Collision Event")
                            }
                        };

                        single_update_player_jump_reset(
                            player_id,
                            other_collider,
                            &jump_reset_query,
                            &mut player_query,
                            CollisionEventType::Stopped,
                        );
                    }
                    CollisionPlayerType::Two(found, (player_one, player_two)) => {
                        double_update_player_jump_reset(
                            player_one,
                            player_two,
                            &jump_reset_query,
                            &child_query,
                            &mut player_query,
                            CollisionEventType::Stopped,
                        );
                    }
                }
            }
        }
    }
}

pub const SETTINGSPATH: &'static str = "game_settings.toml";

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AAWindowSettings {
    pub width: f32,
    pub height: f32,
    pub icon_path: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, Component, Copy)]
pub struct CharacterInputSettings {
    pub up_button: KeyCode,
    pub down_button: KeyCode,
    pub left_button: KeyCode,
    pub right_button: KeyCode,
    pub attack_one: KeyCode,
    pub attack_two: KeyCode,
    pub reset: KeyCode,
}
pub type CharacterInputMap = HashMap<KeyCode, Vec<(u64, InputPurpose)>>;

pub trait InputCharacter {
    fn add_character_inputs(&mut self, input: CharacterInputSettings, id: u64);
}

impl IntoIterator for CharacterInputSettings {
    type Item = (KeyCode, InputPurpose);
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        vec![
            (self.up_button, InputPurpose::Up),
            (self.down_button, InputPurpose::Down),
            (self.left_button, InputPurpose::Left),
            (self.right_button, InputPurpose::Right),
            (self.attack_one, InputPurpose::Atk1),
            (self.attack_two, InputPurpose::Atk2),
            (self.reset, InputPurpose::Reset),
        ]
        .into_iter()
    }
}

impl InputCharacter for CharacterInputMap {
    fn add_character_inputs(&mut self, mut input: CharacterInputSettings, id: u64) {
        for (button, action) in input {
            let current_button_entry = self.entry(button);

            // Set default for the current entry so we can insert into it
            let entry = current_button_entry.or_insert(vec![]);

            entry.push((id, action));
        }
    }
}

#[derive(Inspectable, Debug, Copy, Clone)]
pub enum InputPurpose {
    Up,
    Down,
    Left,
    Right,
    Atk1,
    Atk2,
    Reset,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SpecialSettings {
    pub debug_mode: bool,
    pub vignette: bool,
}

impl Default for SpecialSettings {
    fn default() -> Self {
        Self {
            debug_mode: false,
            vignette: true,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GameplaySettings {
    pub attack_buffer_reset_time: f64,
    pub countdown_to_start_time: f64,
    pub countdown_disappear: f64,
    pub victory_disappear: f64,
    pub rounds_to_map_change: u64,
}

impl Default for GameplaySettings {
    fn default() -> Self {
        Self {
            attack_buffer_reset_time: 3.0,
            countdown_to_start_time: 3.0,
            countdown_disappear: 0.5,
            victory_disappear: 1.0,
            rounds_to_map_change: 3,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GameSettings {
    pub window: AAWindowSettings,
    pub p1_ctrls: CharacterInputSettings,
    pub p2_ctrls: CharacterInputSettings,
    pub p3_ctrls: CharacterInputSettings,
    pub p4_ctrls: CharacterInputSettings,
    pub p5_ctrls: CharacterInputSettings,
    pub p6_ctrls: CharacterInputSettings,
    pub p7_ctrls: CharacterInputSettings,
    pub p8_ctrls: CharacterInputSettings,
    pub special_settings: SpecialSettings,
    pub font_settings: FontSettings,
    pub gameplay_settings: GameplaySettings,
    pub main_menu: MainMenuConfig,
    pub player_id: PlayerIdentifyConfig,
    pub select_char: SelectCharacterConfig,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FontSettings {
    pub bold_path: String,
    pub reg_path: String,
    pub percentage_centre_text_size_to_window_height: f32,
}

impl Default for FontSettings {
    fn default() -> Self {
        FontSettings {
            bold_path: "assets/fonts/kenyan coffee bd.ttf".to_string(),
            reg_path: "assets/fonts/kenyan coffee rg.ttf".to_string(),
            percentage_centre_text_size_to_window_height: 6.4,
        }
    }
}
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PlayerIdentifyConfig {
    pub help_text_vertical: f32,
    pub help_text_size: f32,
    pub help_text_colour: [f32; 3],
    pub player_joined_font_colour: [f32; 3],
    pub player_joined_font_size: f32,
    pub player_joined_window_down: f32,
    pub player_joined_individual_distance: f32,
    pub highlight_colour: [f32; 3],
    pub highlight_height: f32,
    pub bottom_tooltip_height: f32,
    pub bottom_tooltip_colour: [f32; 3],
    pub bottom_tooltip_size: f32,
    pub start_key: KeyCode,
}

impl Default for PlayerIdentifyConfig {
    fn default() -> Self {
        Self {
            help_text_vertical: 0.3,
            help_text_colour: [255.0, 255.0, 255.0],
            help_text_size: 0.08,
            player_joined_font_colour: [255.0, 255.0, 255.0],
            player_joined_font_size: 0.06,
            player_joined_window_down: 0.13,
            player_joined_individual_distance: -0.05,
            highlight_colour: [1.0, 1.0, 1.0],
            highlight_height: 0.08,
            start_key: KeyCode::Space,
            bottom_tooltip_height: -0.4,
            bottom_tooltip_colour: [255.0, 255.0, 255.0],
            bottom_tooltip_size: 0.08,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MainMenuConfig {
    pub main_sprite: String,
    pub main_sprite_window_vertical: f32,
    pub main_sprite_size: [f32; 2],
    pub select_option_font_colour: [f32; 3],
    pub select_option_font_size: f32,
    pub select_option_window_down: f32,
    pub select_option_individual_distance: f32,
    pub highlight_colour: [f32; 3],
    pub highlight_height: f32,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SelectCharacterConfig {
    pub current_player_text_size: f32,
    pub current_player_text_colour: [f32; 3],
    pub current_player_text_up_dist: f32,
    pub current_player_text_highlight_height: f32,
    pub current_player_text_highlight_colour: [f32; 3],
    pub character_icon_size: [f32;2],
    pub icon_vertical_dist: f32,
    pub char_desc_text_size: f32,
    pub char_desc_text_colour: [f32; 3],
    pub char_desc_text_dist: f32,
    pub current_selected_char_text_size: f32,
    pub current_selected_char_colour: [f32; 3],
    pub current_selected_char_up_dist: f32,
    pub current_selected_char_highlight_height: f32,
    pub current_selected_char_highlight_colour: [f32; 3],
}

impl Default for SelectCharacterConfig {
    fn default() -> Self {
        Self {
            current_player_text_size: 0.09,
            current_player_text_colour: [255.0, 255.0, 255.0],
            current_player_text_up_dist: 0.3,
            current_player_text_highlight_height: 0.08,
            current_player_text_highlight_colour: [1.0, 1.0, 1.0],
            character_icon_size: [0.2, 0.2],
            icon_vertical_dist: 0.0,
            char_desc_text_size: 0.08,
            char_desc_text_colour: [255.0, 255.0, 255.0],
            char_desc_text_dist: 0.08,
            current_selected_char_text_size: 0.09,
            current_selected_char_colour: [255.0, 255.0, 255.0],
            current_selected_char_up_dist: -0.2,
            current_selected_char_highlight_height: 0.08,
            current_selected_char_highlight_colour: [1.0, 1.0, 1.0],
        }
    }
}

impl Default for MainMenuConfig {
    fn default() -> Self {
        MainMenuConfig {
            main_sprite: "assets/branding/logo2.png".to_string(),
            main_sprite_window_vertical: 0.25,
            main_sprite_size: [0.35, 0.35],
            select_option_font_colour: [255.0, 255.0, 255.0],
            select_option_font_size: 0.07,
            select_option_window_down: -0.21,
            select_option_individual_distance: -0.06,
            highlight_colour: [212.0, 112.0, 24.0],
            highlight_height: 0.06,
        }
    }
}

impl Default for GameSettings {
    fn default() -> Self {
        Self {
            window: AAWindowSettings::default(),
            p1_ctrls: p1_ctrl_default(),
            p2_ctrls: p2_ctrl_default(),
            p3_ctrls: p3_ctrl_default(),
            p4_ctrls: p4_ctrl_default(),
            p5_ctrls: p5_ctrl_default(),
            p6_ctrls: p6_ctrl_default(),
            p7_ctrls: p7_ctrl_default(),
            p8_ctrls: p8_ctrl_default(),
            special_settings: SpecialSettings::default(),
            font_settings: FontSettings::default(),
            gameplay_settings: GameplaySettings::default(),
            main_menu: MainMenuConfig::default(),
            player_id: PlayerIdentifyConfig::default(),
            select_char: SelectCharacterConfig::default()
        }
    }
}

impl Default for AAWindowSettings {
    fn default() -> Self {
        Self {
            width: 1280.0,
            height: 720.0,
            icon_path: "assets/branding/icon.png".to_string(),
        }
    }
}

pub fn p1_ctrl_default() -> CharacterInputSettings {
    CharacterInputSettings {
        up_button: KeyCode::Key2,
        down_button: KeyCode::W,
        left_button: KeyCode::Q,
        right_button: KeyCode::E,
        attack_one: KeyCode::Key4,
        attack_two: KeyCode::R,
        reset: KeyCode::Key3,
    }
}

pub fn p2_ctrl_default() -> CharacterInputSettings {
    CharacterInputSettings {
        up_button: KeyCode::Key0,
        down_button: KeyCode::P,
        left_button: KeyCode::O,
        right_button: KeyCode::LBracket,
        attack_one: KeyCode::Equals,
        attack_two: KeyCode::RBracket,
        reset: KeyCode::Minus,
    }
}

pub fn p3_ctrl_default() -> CharacterInputSettings {
    CharacterInputSettings {
        up_button: KeyCode::Key6,
        down_button: KeyCode::Y,
        left_button: KeyCode::T,
        right_button: KeyCode::U,
        attack_one: KeyCode::Key8,
        attack_two: KeyCode::I,
        reset: KeyCode::Key7,
    }
}

pub fn p4_ctrl_default() -> CharacterInputSettings {
    CharacterInputSettings {
        up_button: KeyCode::S,
        down_button: KeyCode::X,
        left_button: KeyCode::Z,
        right_button: KeyCode::C,
        attack_one: KeyCode::F,
        attack_two: KeyCode::V,
        reset: KeyCode::D,
    }
}

pub fn p5_ctrl_default() -> CharacterInputSettings {
    CharacterInputSettings {
        up_button: KeyCode::Up,
        down_button: KeyCode::Down,
        left_button: KeyCode::Left,
        right_button: KeyCode::Right,
        attack_one: KeyCode::Numpad2,
        attack_two: KeyCode::Numpad0,
        reset: KeyCode::Numpad1,
    }
}

pub fn p6_ctrl_default() -> CharacterInputSettings {
    CharacterInputSettings {
        up_button: KeyCode::L,
        down_button: KeyCode::Stop,
        left_button: KeyCode::Comma,
        right_button: KeyCode::Slash,
        attack_one: KeyCode::Apostrophe,
        attack_two: KeyCode::RShift,
        reset: KeyCode::Semicolon,
    }
}

pub fn p7_ctrl_default() -> CharacterInputSettings {
    CharacterInputSettings {
        up_button: KeyCode::Home,
        down_button: KeyCode::End,
        left_button: KeyCode::Delete,
        right_button: KeyCode::PageDown,
        attack_one: KeyCode::Numlock,
        attack_two: KeyCode::Numpad7,
        reset: KeyCode::PageUp,
    }
}

pub fn p8_ctrl_default() -> CharacterInputSettings {
    CharacterInputSettings {
        up_button: KeyCode::Numpad5,
        down_button: KeyCode::Numpad2,
        left_button: KeyCode::Numpad1,
        right_button: KeyCode::Numpad3,
        attack_one: KeyCode::NumpadAdd,
        attack_two: KeyCode::NumpadEnter,
        reset: KeyCode::Numpad6,
    }
}

pub fn recieve_settings() -> String {
    match std::fs::read_to_string(SETTINGSPATH) {
        Ok(game_settings) => game_settings,
        Err(_) => {
            let default_settings = GameSettings::default();
            match toml::to_string_pretty(&default_settings) {
                Ok(string_settings) => match std::fs::File::create(SETTINGSPATH) {
                    Ok(mut file) => match write!(file, "{}", string_settings) {
                        Ok(_) => string_settings,
                        Err(error) => {
                            panic!("Error writing default settings to file: {:#?}", error);
                        }
                    },
                    Err(error) => {
                        panic!("Could not create settings file: {:#?}", error);
                    }
                },
                Err(error) => {
                    panic!("Failed to stringify default settings with: {:#?}", error)
                }
            }
        }
    }
}

#[derive(Deserialize, Serialize, Component, Debug, Copy, Clone, Inspectable)]
#[serde(default)]
pub struct VelocityForceCap {
    pub max_velocity: Option<[f32; 2]>,
    pub max_angvel: Option<f32>,
    pub max_external_force: Option<[f32; 2]>,
    pub max_external_torque: Option<f32>,
}

impl Default for VelocityForceCap {
    fn default() -> Self {
        Self {
            max_velocity: None,
            max_angvel: None,
            max_external_force: None,
            max_external_torque: None,
        }
    }
}

pub fn cap_velocity(mut query: Query<(&mut Velocity, &mut ExternalForce, &VelocityForceCap)>) {
    for (mut velocity, mut external_force, cap) in query.iter_mut() {
        velocity.linvel = match cap.max_velocity {
            None => velocity.linvel,
            Some(velocity_cap) => {
                let velocity_cap_vec = Vec2::from(velocity_cap);
                velocity
                    .linvel
                    .clamp(velocity_cap_vec * -1.0, velocity_cap_vec)
            }
        };

        velocity.angvel = match cap.max_angvel {
            None => velocity.angvel,
            Some(angvel_cap) => velocity.angvel.clamp(-angvel_cap, angvel_cap),
        };

        external_force.force = match cap.max_external_force {
            None => external_force.force,
            Some(force_cap) => {
                let force_cap_vec = Vec2::from(force_cap);
                external_force
                    .force
                    .clamp(force_cap_vec * -1.0, force_cap_vec)
            }
        };

        external_force.torque = match cap.max_external_torque {
            None => external_force.torque,
            Some(torque_cap) => external_force.torque.clamp(-torque_cap, torque_cap),
        };
    }
}

#[derive(Copy, Clone, PartialEq)]
pub enum InputKeyboardType {
    JustPressed,
    JustReleased,
    None,
}

pub struct AAGamePlayToggle {
    pub process_movement: bool,
}

pub fn movement_input_system(
    keyboard_input: Res<Input<KeyCode>>,
    input_action_map: Res<CharacterInputMap>,
    mut char_map: ResMut<CharComponentMap>,
    mut commands: Commands,
    mut char_query: Query<(
        &mut Velocity,
        &mut ExternalForce,
        &mut Damping,
        &mut AAPlayerDescriptor,
        &mut AttackBuffer,
        &mut Transform,
    )>,
    mut attack_directory: ResMut<AttackInstanceDirectory>,
    time: Res<Time>,
    settings: Res<GameSettings>,
    svg_test: Res<Assets<Svg>>,
    gameplay_toggle: Res<AAGamePlayToggle>,
) {
    if !gameplay_toggle.process_movement {
        return;
    };

    for (keycode, actions) in input_action_map.iter() {
        let input_type = if keyboard_input.just_pressed(*keycode) {
            println!("{:?} {:?}", keycode, actions);
            InputKeyboardType::JustPressed
        } else if keyboard_input.just_released(*keycode) {
            InputKeyboardType::JustReleased
        } else {
            InputKeyboardType::None
        };

        for action in actions {
            let player_id = action.0;
            let mut char_components = match char_map.get_mut(&player_id) {
                None => {
                    panic!("Got input for non existant player id");
                }
                Some(components) => components,
            };

            match char_query.get_mut(char_components.core) {
                Ok((
                    mut velocity,
                    mut external_force,
                    mut damping,
                    mut player_descriptor,
                    mut attack_buffer,
                    mut transform,
                )) => {
                    // Attack Actions
                    attack_buffer.add_to_buffer(input_type, action.1, &time);
                    attack_buffer.execute_buffer(
                        &mut char_components,
                        &mut player_descriptor,
                        &mut transform,
                        &mut commands,
                        &mut attack_directory,
                        &time,
                        &settings,
                    );

                    // Movement Actions
                    char_components.actions.apply_action(
                        input_type,
                        action.1,
                        velocity,
                        external_force,
                        damping,
                        player_descriptor,
                    );
                }
                Err(error) => {}
            };
        }
    }
}

bitflags::bitflags! {
    pub struct FullDirectionFacingFlags: u32 {
        const NONE = 0b00000001;
        const UP = 0b00000010;
        const LEFT = 0b00000100;
        const RIGHT = 0b00001000;
        const DOWN = 0b00010000;
        const UP_LEFT = Self::UP.bits | Self::LEFT.bits;
        const UP_RIGHT = Self::UP.bits | Self::RIGHT.bits;
        const DOWN_LEFT = Self::DOWN.bits | Self::LEFT.bits;
        const DOWN_RIGHT = Self::DOWN.bits | Self::RIGHT.bits;
    }
}

#[derive(Component, Copy, Clone, Debug, Serialize, Deserialize)]
pub struct DirRotateAngles {
    pub up: f32,
    pub down: f32,
    pub left: f32,
    pub right: f32,
    pub up_left: f32,
    pub up_right: f32,
    pub down_left: f32,
    pub down_right: f32,
    pub radians: bool,
}

impl Default for DirRotateAngles {
    fn default() -> Self {
        Self {
            down: 270.0,
            right: 0.0,
            left: 180.0,
            up: 90.0,
            down_right: 315.0,
            down_left: 225.0,
            up_right: 45.0,
            up_left: 135.0,
            radians: false,
        }
    }
}

impl DirRotateAngles {
    fn get_radians(&self) -> Self {
        match self.radians {
            true => *self,
            false => Self {
                down: self.down.to_radians(),
                right: self.right.to_radians(),
                left: self.left.to_radians(),
                up: self.up.to_radians(),
                down_right: self.down_right.to_radians(),
                down_left: self.down_left.to_radians(),
                up_right: self.up_right.to_radians(),
                up_left: self.up_left.to_radians(),
                radians: true,
            },
        }
    }
}

impl FullDirectionFacingFlags {
    pub fn rotation_angle(&self, dir_angles: DirRotateAngles) -> f32 {
        let angles = dir_angles.get_radians();
        (match self {
            &Self::UP_LEFT => angles.up_left,
            &Self::UP_RIGHT => angles.up_right,
            &Self::DOWN_LEFT => angles.down_left,
            &Self::DOWN_RIGHT => angles.down_right,
            &Self::UP => angles.up,
            &Self::LEFT => angles.left,
            &Self::RIGHT => angles.right,
            &Self::DOWN => angles.down,
            _ => 0.0_f32.to_radians(),
        }) as f32
    }
}

impl DirectionFacingFlags {
    pub fn get_full_dir_flags(&self) -> FullDirectionFacingFlags {
        // check for complements
        let mut full_dir_flags: FullDirectionFacingFlags = FullDirectionFacingFlags::NONE;
        full_dir_flags.bits = self.bits();

        if full_dir_flags.contains(FullDirectionFacingFlags::UP)
            & full_dir_flags.contains(FullDirectionFacingFlags::DOWN)
        {
            full_dir_flags.set(FullDirectionFacingFlags::UP, false);
            full_dir_flags.set(FullDirectionFacingFlags::DOWN, false);
        };

        if full_dir_flags.contains(FullDirectionFacingFlags::LEFT)
            & full_dir_flags.contains(FullDirectionFacingFlags::RIGHT)
        {
            full_dir_flags.set(FullDirectionFacingFlags::LEFT, false);
            full_dir_flags.set(FullDirectionFacingFlags::RIGHT, false);
        };

        if full_dir_flags.intersects(!FullDirectionFacingFlags::NONE) {
            full_dir_flags.set(FullDirectionFacingFlags::NONE, false)
        }

        full_dir_flags
    }
}
