use bitflags::bitflags;
use std::collections::HashMap;
use std::ops::{Add, Index};
use std::path::PathBuf;

use crate::action_traits::ScaleToRatio;
use crate::assets::*;
use crate::collider::*;
use crate::game::*;
use crate::projectile::{
    create_projectile_map, Attack, AttackActions, AttackMap, ExecuteOptionAttack, InstanceMap,
    MovementAttackAction, Projectile, ProjectileIdentifier, ProjectileMap, SpawnProjectileAction,
    UnusedAction,
};
use crate::rigidbody::*;
use crate::universal::*;
use crate::{
    AppStates, AssetServer, CharacterInputIdentifierMap, CharacterInputSettings, CountDownTextNode,
    DirRotateAngles, EntityPath, Game, GameRounds, GameSettings, InputKeyboardType, Map, Mut,
    QuerySingleError, Res, ResMut, SvgInfo, VelocityForceCap,
};
use bevy::asset::HandleId;
use bevy::ecs::component::Component;
use bevy::ecs::query::QueryEntityError;
use bevy::ecs::system::EntityCommands;
use bevy::log::error;
use bevy::log::warn;
use bevy::math::Mat2;
use bevy::math::{Quat, Vec3, Vec3Swizzles};
use bevy::prelude::{
    Assets, BuildChildren, ChildBuilder, Color, ColorMaterial, Commands, DespawnRecursiveExt,
    Entity, Handle, HandleUntyped, HorizontalAlign, Image, Mesh, Query, Sprite, SpriteBundle,
    State, Text, Text2dBundle, TextAlignment, TextStyle, Time, Transform, Vec2, VerticalAlign,
    With, Without,
};
use bevy::sprite::MaterialMesh2dBundle;
use bevy::sprite::Mesh2dHandle;
use bevy::tasks::AsyncComputeTaskPool;
use bevy::window::WindowDescriptor;
use bevy_inspector_egui::Inspectable;
use bevy_mod_rounded_box::RoundedBox;
use bevy_rapier2d::prelude::*;
use bevy_rapier2d::rapier::prelude::{
    ColliderShape, ColliderType, RigidBodyPosition, RigidBodyType,
};
use bevy_rapier2d::render::*;
use bevy_svg::prelude::{Origin, Svg};
use rand;
use rand::Rng;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct CharComponent {
    pub asset: String,

    #[serde(default)]
    pub character_assets: Vec<String>,

    #[serde(default = "f32::default")]
    pub rotation: f32,

    #[serde(default = "nz_vecfault")]
    pub scale: [f32; 3],

    #[serde(default = "z_vecfault")]
    pub origin: [f32; 3],

    #[serde(default = "String::default")]
    pub physobj_id: String,

    #[serde(default = "default_true")]
    pub enable_sync_rotation: bool,

    #[serde(default)]
    pub face_dir_looking_radius: f32,

    #[serde(default)]
    pub face_dir_looking_angles: DirRotateAngles,
}

impl Default for DirectionFacingFlags {
    fn default() -> Self {
        DirectionFacingFlags::NONE
    }
}

#[derive(Debug, Clone, Copy, Component, Default)]
pub struct PlayerHealthBarId {
    pub player_id: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, Component, Default, Inspectable)]
#[serde(default)]
pub struct PlayerHealth {
    pub maximum_health: f32,

    #[serde(skip)]
    pub current_health: f32,

    pub healthbar_distance: f32,
    pub width: f32,
    pub height: f32,
    pub radius: f32,
    pub subdivisions: usize,
}

#[derive(Serialize, Deserialize, Debug, Clone, Component, Default)]
pub struct AvailableAttacks {
    pub one_one: Option<Attack>,
    pub one_two: Option<Attack>,
    pub two_one: Option<Attack>,
    pub two_two: Option<Attack>,
}

#[derive(Debug, Clone, Component, Copy)]
pub enum AttackKey {
    One,
    Two,
}

#[derive(Debug, Clone, Component, Default)]
pub struct AttackBuffer {
    pub buffer: Vec<AttackKey>,
    pub final_timestamp: f64,
}

impl Default for AttackKey {
    fn default() -> Self {
        Self::One
    }
}

pub type CooldownMap = HashMap<u64, HashMap<AttackType, AttackCooldown>>;

pub struct AttackInstanceDirectory {
    pub attack_instances: HashMap<u64, InstanceMap>,
    pub previous_used_id: u64,
    pub unexecuted_actions: Vec<UnusedAction>,
    pub cooldown: HashMap<u64, HashMap<AttackType, AttackCooldown>>,
}

pub fn check_cooldowns(
    mut attack_instance_directory: ResMut<AttackInstanceDirectory>,
    game: Res<Game>,
    time: Res<Time>,
) {
    for player in game.selected_characters.keys() {
        let cooldowns = attack_instance_directory
            .cooldown
            .entry(*player)
            .or_insert(HashMap::new());
        cooldowns.retain(|attack_type, cooldown| {
            if cooldown.cooldown_duration - (time.seconds_since_startup() - cooldown.cooldown_start)
                <= 0.0
            {
                false
            } else {
                true
            }
        });
    }
}

pub fn execute_attack<T: ExecuteOptionAttack>(
    attack: T,
    char_entities: &mut CharEntities,
    player_descriptor: &mut AAPlayerDescriptor,
    transform: &mut Transform,
    commands: &mut Commands,
    attack_directory: &mut ResMut<AttackInstanceDirectory>,
    time: &Res<Time>,
) {
    attack.execute_attack(
        char_entities,
        player_descriptor,
        transform,
        commands,
        attack_directory,
        time,
    )
}

pub struct AttackCooldown {
    pub cooldown_start: f64,
    pub cooldown_duration: f64,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum AttackType {
    OneOne,
    TwoOne,
    OneTwo,
    TwoTwo,
}

impl CharEntities {
    pub fn apply_attack(
        &mut self,
        buffer: [AttackKey; 2],
        mut player_descriptor: &mut AAPlayerDescriptor,
        transform: &mut Transform,
        commands: &mut Commands,
        attack_directory: &mut ResMut<AttackInstanceDirectory>,
        time: &Res<Time>,
    ) {
        let (attack, attack_type) = match buffer {
            [AttackKey::One, AttackKey::One] => {
                (&self.attacks.one_one, AttackType::OneOne)
            }
            [AttackKey::One,AttackKey::Two] => {
                (&self.attacks.one_two, AttackType::OneTwo)
            }
            [ AttackKey::Two, AttackKey::One] => {
                (&self.attacks.two_one, AttackType::TwoOne)
            }
            [AttackKey::Two,  AttackKey::Two] => {
                (&self.attacks.two_two, AttackType::TwoTwo)
            }
            _ => return,
        };

        let cooldown_player_id = attack_directory
            .cooldown
            .entry(self.player_id)
            .or_insert(HashMap::new());

        match cooldown_player_id.get(&attack_type) {
            None => {
                match attack.clone() {
                    None => {}
                    Some(attack) => {
                        //Start the cooldown for the attack we just executed (uh oh)
                        cooldown_player_id.insert(
                            attack_type,
                            AttackCooldown {
                                cooldown_start: time.seconds_since_startup(),
                                cooldown_duration: attack.cooldown,
                            },
                        );
                    }
                }

                execute_attack(
                    attack.clone(),
                    self,
                    player_descriptor,
                    transform,
                    commands,
                    attack_directory,
                    time,
                );
            }
            _ => return,
        }
    }
}

impl AttackBuffer {
    pub fn add_to_buffer(
        &mut self,
        input_type: InputKeyboardType,
        attack_key: InputPurpose,
        time: &Res<Time>,
    ) {
        if input_type == InputKeyboardType::JustPressed {
            match attack_key {
                InputPurpose::Atk1 => {
                    self.buffer.push(AttackKey::One);
                }
                InputPurpose::Atk2 => {
                    self.buffer.push(AttackKey::Two);
                }
                InputPurpose::Reset => {
                    self.buffer.clear();
                    self.final_timestamp = 0.0;
                    return;
                }
                _ => return,
            };

            self.final_timestamp = time.seconds_since_startup();
        }
    }

    pub fn execute_buffer(
        &mut self,
        mut char_entities: &mut CharEntities,
        mut player_descriptor: &mut AAPlayerDescriptor,
        transform: &mut Transform,
        commands: &mut Commands,
        attack_directory: &mut ResMut<AttackInstanceDirectory>,
        time: &Res<Time>,
        settings: &Res<GameSettings>,
    ) {
        if (self.final_timestamp + settings.gameplay_settings.attack_buffer_reset_time)
            < time.seconds_since_startup()
        {
            self.buffer.clear();
            self.final_timestamp = 0.0;
        };

        if self.buffer.len() >= 2 {
            let attack_buffer = [
                *self.buffer.get(0).unwrap_or(&AttackKey::default()),
                *self.buffer.get(1).unwrap_or(&AttackKey::default()),
            ];

            self.buffer.clear();
            self.final_timestamp = 0.0;

            char_entities.apply_attack(
                attack_buffer,
                player_descriptor,
                transform,
                commands,
                attack_directory,
                time,
            );
        }
    }
}

bitflags! {
    pub struct DirectionFacingFlags: u32 {
        const NONE = 0b00000001;
        const UP = 0b00000010;
        const LEFT = 0b00000100;
        const RIGHT = 0b00001000;
        const DOWN = 0b00010000;
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Component)]
pub struct AAPlayerDescriptor {
    #[serde(default = "one_u32fault")]
    pub maximum_jumps: u32,

    #[serde(skip_deserializing)]
    pub available_jumps: u32,

    #[serde(skip_deserializing)]
    pub char_collision_dominance: bool,

    #[serde(skip)]
    pub direction_facing: DirectionFacingFlags,

    #[serde(skip)]
    pub lock_jumps_at_max: bool,
}

impl Default for AAPlayerDescriptor {
    fn default() -> Self {
        AAPlayerDescriptor {
            maximum_jumps: 1,
            available_jumps: 0,
            char_collision_dominance: false,
            direction_facing: DirectionFacingFlags::NONE,
            lock_jumps_at_max: false,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Component, Default)]
#[serde(default)]
pub struct MovementAction {
    pub id: String,
    pub instance_affected_id: String,
    pub set_velocity: Option<[f32; 2]>,
    pub add_velocity: Option<[f32; 2]>,
    pub set_angvel: Option<f32>,
    pub add_angvel: Option<f32>,
    pub set_external_force: Option<[f32; 2]>,
    pub add_external_force: Option<[f32; 2]>,
    pub set_external_torque: Option<f32>,
    pub add_external_torque: Option<f32>,
    pub set_lin_damping: Option<f32>,
    pub add_lin_damping: Option<f32>,
    pub set_ang_damping: Option<f32>,
    pub add_ang_damping: Option<f32>,
    pub require_available_jumps: bool,

    #[serde(default = "one_u32fault")]
    pub required_jumps: u32,

    #[serde(default = "one_u32fault")]
    pub jumps_removed: u32,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(default)]
pub struct AvailableMovementActions {
    pub pressed_up: MovementAction,
    pub pressed_down: MovementAction,
    pub pressed_left: MovementAction,
    pub pressed_right: MovementAction,
    pub unpressed_up: MovementAction,
    pub unpressed_down: MovementAction,
    pub unpressed_left: MovementAction,
    pub unpressed_right: MovementAction,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, Copy)]
#[serde(default)]
pub struct PlayerIdText {
    pub distance: f32,
    pub size: f32,
}

impl MovementAction {
    pub fn set_values(
        &self,
        velocity: &mut Mut<Velocity>,
        external_force: &mut Mut<ExternalForce>,
        damping: &mut Mut<Damping>,
    ) {
        velocity.linvel = match self.set_velocity {
            None => velocity.linvel,
            Some(velocity) => Vec2::from(velocity),
        };

        velocity.angvel = match self.set_angvel {
            None => velocity.angvel,
            Some(angvel) => angvel,
        };

        external_force.force = match self.set_external_force {
            None => external_force.force,
            Some(force) => Vec2::from(force),
        };

        external_force.torque = match self.set_external_torque {
            None => external_force.torque,
            Some(torque) => torque,
        };

        damping.linear_damping = match self.set_lin_damping {
            None => damping.linear_damping,
            Some(lin_damping) => lin_damping,
        };

        damping.angular_damping = match self.set_ang_damping {
            None => damping.angular_damping,
            Some(ang_damping) => ang_damping,
        };
    }
    pub fn add_values(
        &self,
        velocity: &mut Mut<Velocity>,
        external_force: &mut Mut<ExternalForce>,
        damping: &mut Mut<Damping>,
    ) {
        velocity.linvel += match self.add_velocity {
            None => Vec2::default(),
            Some(velocity) => Vec2::from(velocity),
        };

        velocity.angvel += match self.add_angvel {
            None => 0.0,
            Some(angvel) => angvel,
        };

        external_force.force += match self.add_external_force {
            None => Vec2::default(),
            Some(force) => Vec2::from(force),
        };

        external_force.torque += match self.add_external_torque {
            None => 0.0,
            Some(torque) => torque,
        };

        damping.linear_damping += match self.add_lin_damping {
            None => 0.0,
            Some(lin_damping) => lin_damping,
        };

        damping.angular_damping += match self.add_ang_damping {
            None => 0.0,
            Some(ang_damping) => ang_damping,
        };
    }
    pub fn apply_action(
        &mut self,
        mut velocity: Mut<Velocity>,
        mut external_force: Mut<ExternalForce>,
        mut damping: Mut<Damping>,
        mut player_descriptor: Mut<AAPlayerDescriptor>,
        mut set_direction_facing: (DirectionFacingFlags, bool),
    ) {
        player_descriptor
            .direction_facing
            .set(set_direction_facing.0, set_direction_facing.1);

        let process_actions = match self.require_available_jumps {
            //Check if there is enough available jumps to process this action & if there is then decrease it
            // Check if flag should allow action to happen or not
            true => {
                if player_descriptor.available_jumps >= self.required_jumps {
                    if !player_descriptor.char_collision_dominance {
                        player_descriptor.available_jumps -= self.jumps_removed;
                    }
                    true
                } else {
                    false
                }
            }
            false => true,
        };

        if process_actions {
            self.set_values(&mut velocity, &mut external_force, &mut damping);
            self.add_values(&mut velocity, &mut external_force, &mut damping);
        }
    }
}

impl AvailableMovementActions {
    pub fn apply_action(
        &mut self,
        action_type: InputKeyboardType,
        action: InputPurpose,
        mut velocity: Mut<Velocity>,
        mut external_force: Mut<ExternalForce>,
        mut damping: Mut<Damping>,
        mut player_descriptor: Mut<AAPlayerDescriptor>,
    ) {
        match action_type {
            InputKeyboardType::JustPressed => {
                match action {
                    InputPurpose::Up => {
                        self.pressed_up.apply_action(
                            velocity,
                            external_force,
                            damping,
                            player_descriptor,
                            (DirectionFacingFlags::UP, true),
                        );
                    }
                    InputPurpose::Down => {
                        self.pressed_down.apply_action(
                            velocity,
                            external_force,
                            damping,
                            player_descriptor,
                            (DirectionFacingFlags::DOWN, true),
                        );
                    }
                    InputPurpose::Left => {
                        self.pressed_left.apply_action(
                            velocity,
                            external_force,
                            damping,
                            player_descriptor,
                            (DirectionFacingFlags::LEFT, true),
                        );
                    }
                    InputPurpose::Right => {
                        self.pressed_right.apply_action(
                            velocity,
                            external_force,
                            damping,
                            player_descriptor,
                            (DirectionFacingFlags::RIGHT, true),
                        );
                    }
                    _ => {}
                };
            }
            InputKeyboardType::JustReleased => {
                match action {
                    InputPurpose::Up => {
                        self.unpressed_up.apply_action(
                            velocity,
                            external_force,
                            damping,
                            player_descriptor,
                            (DirectionFacingFlags::UP, false),
                        );
                    }
                    InputPurpose::Down => {
                        self.unpressed_down.apply_action(
                            velocity,
                            external_force,
                            damping,
                            player_descriptor,
                            (DirectionFacingFlags::DOWN, false),
                        );
                    }
                    InputPurpose::Left => {
                        self.unpressed_left.apply_action(
                            velocity,
                            external_force,
                            damping,
                            player_descriptor,
                            (DirectionFacingFlags::LEFT, false),
                        );
                    }
                    InputPurpose::Right => {
                        self.unpressed_right.apply_action(
                            velocity,
                            external_force,
                            damping,
                            player_descriptor,
                            (DirectionFacingFlags::RIGHT, false),
                        );
                    }
                    _ => {}
                };
            }
            InputKeyboardType::None => {}
        }
    }
}

pub struct CharEntities {
    pub core: Entity,
    pub static_component: Option<Vec<Entity>>,
    pub actions: AvailableMovementActions,
    pub attacks: AvailableAttacks,
    pub projectiles: ProjectileMap,
    pub rescale_ratio: Vec3,
    pub attack_actions: AttackMap,
    pub physics_map: PhysMap,
    pub collider_map: ColliderMap,
    pub asset_map: AssetMap,
    pub player_id: u64,
    pub movement_map: MovementActionMap,
}
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Character {
    pub info: Info,
    pub core: CharComponent,

    #[serde(default)]
    pub char_component: Vec<CharComponent>,

    #[serde(default = "PathBuf::default", skip_deserializing)]
    pub base_path: PathBuf,

    #[serde(default = "Asset::default")]
    pub asset: Asset,

    #[serde(default = "AACollider::default")]
    pub collider: AACollider,

    #[serde(default = "Vec::default")]
    pub physics_object: Vec<PhysicsObject>,

    #[serde(default)]
    pub player_descriptor: AAPlayerDescriptor,

    #[serde(default)]
    pub velocity_cap: VelocityForceCap,

    #[serde(default)]
    pub char_movement_action: AvailableMovementActions,

    #[serde(default)]
    pub health: PlayerHealth,

    #[serde(default)]
    pub text_identifier: PlayerIdText,

    #[serde(default)]
    pub attack_identifier: PlayerIdText,

    #[serde(default)]
    pub attack: AvailableAttacks,

    #[serde(default)]
    pub projectile: Vec<Projectile>,

    #[serde(default)]
    pub spawn_projectile_action: Vec<SpawnProjectileAction>,

    #[serde(default)]
    pub movement_action: Vec<MovementAction>,

    #[serde(default)]
    pub movement_attack_action: Vec<MovementAttackAction>,
}

pub struct CharIdentifier(pub u64);

impl PossibleBundleRetrieve for CharComponent {
    fn retrieve_bundle(&self, assets: &AssetMap, ratio: Vec3) -> PossibleBundle {
        retrieve_possible_bundle(
            assets,
            self.scale,
            self.origin,
            self.rotation,
            &self.asset,
            ratio,
        )
    }
}

impl PossibleBundleRetrieveWithAsset for CharComponent {
    fn retrieve_bundle_with_asset(
        &self,
        assets: &AssetMap,
        ratio: Vec3,
        asset: String,
    ) -> PossibleBundle {
        retrieve_possible_bundle(
            assets,
            self.scale,
            self.origin,
            self.rotation,
            &asset,
            ratio,
        )
    }
}

#[derive(Component, Inspectable)]
pub struct PlayerIdentifier {
    pub player_id: u64,
}
impl PathAdjust for Character {
    fn change_path(&mut self, new_path: PathBuf) {
        self.base_path = new_path
    }
}

pub fn load_characters(mut commands: Commands) {
    let mut chars: Vec<Character> = vec![];
    println!("Loading characters...");
    load_directory("char".into(), "main.toml", &mut chars);
    println!("Loaded  characters");
    commands.insert_resource(chars);
}

impl TransmuteAsset for Character {
    fn transmute_assets(
        &self,
        asset_server: &Res<AssetServer>,
        interpolate_handles: &mut ResMut<InterpolateHandles>,
    ) -> (AssetMap, HandleIdVec) {
        load_assets(
            &self.asset,
            self.base_path.clone(),
            asset_server,
            Some(interpolate_handles),
        )
    }
}

#[derive(Debug, Copy, Clone)]
pub enum VictoryEvent {
    Victory(u64),
    Draw,
    None,
}

pub type CharComponentMap = HashMap<u64, CharEntities>;

pub fn check_victory_conditions(
    player_query: Query<(&PlayerIdentifier)>,
    mut commands: Commands,
    mut state: ResMut<State<AppStates>>,
    mut game_rounds: ResMut<GameRounds>,
) {
    let player_iterator = player_query.iter();

    let victory_event = match player_iterator.len() {
        0 => VictoryEvent::Draw,
        1 => {
            let (player_id) = match player_query.get_single() {
                Ok(player_id) => player_id,
                Err(error) => {
                    warn!("Unable to get victory query for player {:#?}", error);
                    return;
                }
            };

            VictoryEvent::Victory(player_id.player_id)
        }
        _ => {
            return;
        }
    };

    game_rounds.previous_victory = victory_event;
    game_rounds.total_victories.push(victory_event);
    state.set(AppStates::Victory);
}

pub fn load_selected_characters(
    mut game: ResMut<Game>,
    asset_dir: Res<AssetDirectory>,
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    asset_information: Res<AssetInfoMap>,
    window_descriptor: Res<WindowDescriptor>,
    character_input_settings_map: Res<CharacterInputIdentifierMap>,
    mut state: ResMut<State<AppStates>>,
    mut controls_map: ResMut<CharacterInputMap>,
    mut coreponents: ResMut<CharComponentMap>,
    game_settings: Res<GameSettings>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    fonts: Res<AugmentedFonts>,
) {
    let window_dimensions = Vec2::new(window_descriptor.width, window_descriptor.height);

    let map_dim = Vec2::from(game.selected_map.info.base_dimensions);
    let spawn_positions = game.selected_map.spawn_positions.positions.clone();
    let map_screen_ratio = Vec2::from(game.selected_map.info.base_dimensions) / window_dimensions;
    for (index, character) in &game.selected_characters {
        let map_character_ratio = map_dim / Vec2::from(character.info.base_dimensions);
        let character_screen_ratio = map_character_ratio * map_screen_ratio;
        let asset_map = asset_dir.char_assets.get(&index).unwrap();
        let character_controls = *character_input_settings_map.map.get(&index).unwrap();
        controls_map.add_character_inputs(character_controls, *index);

        let spawn_pos = match spawn_positions.get((*index - 1) as usize) {
            Some(pos) => Vec2::from(*pos),
            None => Vec2::new(0.0, 0.0),
        };

        let spawn_transform =
            Transform::from_translation((spawn_pos / character_screen_ratio.y).extend(0.0));

        let char_entity = load_character(
            character,
            asset_map.clone(),
            &mut commands,
            &asset_server,
            character_screen_ratio.extend(1.0),
            *index,
            &game_settings,
            spawn_transform,
            &mut meshes,
            &mut materials,
            &fonts,
            &game.selected_map,
        );

        coreponents.insert(*index, char_entity);
    }
    state.set(AppStates::PreGame);
}

#[derive(Component, Deserialize, Debug, Clone)]
pub struct MoveDirLookIdentifier {
    pub move_dir_radius: f32,
    pub dir_angles: DirRotateAngles,
}

#[derive(Component, Deserialize, Debug, Clone)]
pub struct CharComponentPlayerIdentifier {
    pub player_id: u64,
}

pub enum CharComponentIdentifiers {
    IDMoveDirLookIdentifier(MoveDirLookIdentifier),
    IDCharComponentPlayerIdentifier(CharComponentPlayerIdentifier),
}

impl CharComponent {
    pub fn spawn_self(
        &self,
        assets: &AssetMap,
        base_info: &Info,
        ratio: &Vec3,
        player_id: u64,
        commands: &mut Commands,
        offset_transform: SyncTransformOffset,
        extra_identifiers: Vec<CharComponentIdentifiers>,
    ) -> Entity {
        let possible_bundle = match self.character_assets.get((player_id - 1) as usize) {
            Some(asset_id) => self.retrieve_bundle_with_asset(assets, *ratio, asset_id.clone()),
            None => self.retrieve_bundle(assets, *ratio),
        };

        let mut spawned_bundle = match possible_bundle {
            PossibleBundle::Sprite(spritebundle) => commands.spawn_bundle(spritebundle),
            PossibleBundle::Svg(svgbundle) => commands.spawn_bundle(svgbundle),
        };

        spawned_bundle.insert(offset_transform);

        for possible_identifier in extra_identifiers {
            match possible_identifier {
                CharComponentIdentifiers::IDMoveDirLookIdentifier(identifier) => {
                    spawned_bundle.insert(identifier);
                }
                CharComponentIdentifiers::IDCharComponentPlayerIdentifier(identifier) => {
                    spawned_bundle.insert(identifier);
                }
            }
        }
        spawned_bundle.id()
    }
}

pub fn get_character_controls(
    player_id: u64,
    game_settings: &Res<GameSettings>,
) -> CharacterInputSettings {
    match player_id {
        1 => game_settings.p1_ctrls.clone(),
        2 => game_settings.p2_ctrls.clone(),
        3 => game_settings.p3_ctrls.clone(),
        4 => game_settings.p4_ctrls.clone(),
        5 => game_settings.p5_ctrls.clone(),
        6 => game_settings.p6_ctrls.clone(),
        7 => game_settings.p7_ctrls.clone(),
        8 => game_settings.p8_ctrls.clone(),
        _ => {
            panic!("unnacceptable player id")
        }
    }
}

pub fn spawn_core(
    char: &Character,
    rescale_ratio: &Vec3,
    player_id: u64,
    assets: &AssetMap,
    mut commands: &mut Commands,
    phys_map: &PhysMap,
    mut colliders: &mut ColliderMap,
    spawn_transform: Transform,
    mut extra_sync: Vec<(Entity, SyncColliderFlags)>,
) -> Entity {
    let core = char.core.spawn_self(
        &assets,
        &char.info,
        &rescale_ratio,
        player_id,
        &mut commands,
        SyncTransformOffset::default(),
        vec![],
    );
    let player_identifier = PhysicsSpawnExtras::PlayerIdentifier(PlayerIdentifier { player_id });
    let player_descriptor = PhysicsSpawnExtras::AAPlayerDescriptor(char.player_descriptor.clone());
    let mut player_health = char.health;
    player_health.current_health = player_health.maximum_health;
    let mut rigid_extras = vec![
        player_identifier,
        player_descriptor,
        PhysicsSpawnExtras::VelocityCap(char.velocity_cap),
        PhysicsSpawnExtras::SpawnTransform(spawn_transform),
        PhysicsSpawnExtras::PlayerHP(player_health),
        PhysicsSpawnExtras::AttackBuffer(AttackBuffer {
            buffer: vec![],
            final_timestamp: 0.0,
        }),
    ];

    extra_sync.push((
        core,
        SyncColliderFlags {
            rotation: char.core.enable_sync_rotation,
        },
    ));

    match phys_map.spawn_physics_object_with_sync(
        &char.core.physobj_id,
        extra_sync,
        &mut colliders,
        &mut commands,
        None,
        rigid_extras,
        rescale_ratio,
    ) {
        None => {
            panic!("no phys object under core, useless character no point lol")
        }
        Some(core_phys) => core_phys,
    }
}

pub fn load_char_components(
    mut phys_map: &mut PhysMap,
    rescale_ratio: &Vec3,
    assets: &AssetMap,
    mut commands: &mut Commands,
    char: &Character,
    player_id: u64,
) {
    for char_component in &char.char_component {
        match phys_map.get_mut(&char_component.physobj_id) {
            None => {}
            Some(phys_object) => {
                let mut extra_identifiers = vec![];
                match char_component.face_dir_looking_radius {
                    0.0 => {}
                    _ => {
                        extra_identifiers.push(CharComponentIdentifiers::IDMoveDirLookIdentifier(
                            MoveDirLookIdentifier {
                                move_dir_radius: char_component.face_dir_looking_radius
                                    / rescale_ratio.y,
                                dir_angles: char_component.face_dir_looking_angles,
                            },
                        ));
                    }
                }

                extra_identifiers.push(CharComponentIdentifiers::IDCharComponentPlayerIdentifier(
                    CharComponentPlayerIdentifier {
                        player_id: player_id,
                    },
                ));

                phys_object.extra_synced_items.push((
                    char_component.spawn_self(
                        &assets,
                        &char.info,
                        &rescale_ratio,
                        player_id,
                        &mut commands,
                        SyncTransformOffset::default(),
                        extra_identifiers,
                    ),
                    SyncColliderFlags {
                        rotation: char_component.enable_sync_rotation,
                    },
                ));
            }
        }
    }
}

pub fn create_healthbar(
    char: &Character,
    mut commands: &mut Commands,
    ratio: Vec3,
    mut meshes: &mut ResMut<Assets<Mesh>>,
    mut materials: &mut ResMut<Assets<ColorMaterial>>,
    map: &Map,
    player_id: u64,
) -> Entity {
    commands
        .spawn_bundle(MaterialMesh2dBundle {
            mesh: meshes
                .add(Mesh::from(RoundedBox {
                    size: Vec3::new(
                        char.health.width / ratio.y,
                        char.health.height / ratio.y,
                        1.,
                    ),
                    radius: char.health.radius / ratio.y,
                    subdivisions: char.health.subdivisions,
                }))
                .into(),

            transform: Transform::from_translation(Vec3::new(0.0, 0.0, 997.0)),
            material: materials.add(ColorMaterial::from(Color::from(
                map.char_element_colours
                    .healthbar_max
                    // Conversion from u8 RGB to between 0.0 and 1.0
                    .convert_to_rgb(),
            ))),
            ..Default::default()
        })
        .insert(SyncTransformOffset {
            transform: Transform::from_translation(
                Vec3::new(0., char.health.healthbar_distance, 0.) / ratio.y,
            ),
        })
        .insert(PlayerHealthBarId { player_id })
        .id()
}

pub trait ConvertToRgb {
    fn convert_to_rgb(self) -> Self;
}

impl ConvertToRgb for [f32; 3] {
    fn convert_to_rgb(self) -> Self {
        self.map(|colour| colour / 255.0)
    }
}

pub fn create_player_id_text(
    char: &Character,
    mut commands: &mut Commands,
    ratio: Vec3,
    player_id: u64,
    font: &Res<AugmentedFonts>,
    map: &Map,
) -> Entity {
    let text_style = TextStyle {
        font: font.regular_font.clone(),
        font_size: char.text_identifier.size / ratio.y,
        color: Color::from(map.char_element_colours.player_id_text.convert_to_rgb()),
    };

    let text_alignment = TextAlignment {
        vertical: VerticalAlign::Center,
        horizontal: HorizontalAlign::Center,
    };

    commands
        .spawn_bundle(Text2dBundle {
            text: Text::with_section(
                format!("P{:}", player_id),
                text_style.clone(),
                text_alignment,
            ),
            transform: Transform::from_translation(Vec3::new(0.0, 0.0, 998.0)),
            ..Default::default()
        })
        .insert(SyncTransformOffset {
            transform: Transform::from_translation(
                Vec3::new(0., char.text_identifier.distance, 0.) / ratio.y,
            ),
        })
        .id()
}

#[derive(Clone, Debug, Component, Inspectable)]
pub struct AttackIdentifierTextId {
    pub player_id: u64,
    pub text_style: TextStyle,
    pub text_alignment: TextAlignment,
}

pub fn create_player_atk_text(
    char: &Character,
    mut commands: &mut Commands,
    ratio: Vec3,
    player_id: u64,
    font: &Res<AugmentedFonts>,
    map: &Map,
) -> Entity {
    let text_style = TextStyle {
        font: font.regular_font.clone(),
        font_size: char.attack_identifier.size / ratio.y,
        color: Color::from(map.char_element_colours.player_id_text.convert_to_rgb()),
    };

    let text_alignment = TextAlignment {
        vertical: VerticalAlign::Center,
        horizontal: HorizontalAlign::Center,
    };

    commands
        .spawn_bundle(Text2dBundle {
            text: Text::with_section(format!(""), text_style.clone(), text_alignment),
            transform: Transform::from_translation(Vec3::new(0.0, 0.0, 998.0)),
            ..Default::default()
        })
        .insert(SyncTransformOffset {
            transform: Transform::from_translation(
                Vec3::new(0., char.attack_identifier.distance, 0.) / ratio.y,
            ),
        })
        .insert(AttackIdentifierTextId {
            player_id,
            text_style,
            text_alignment,
        })
        .id()
}

pub type MovementActionMap = HashMap<String, Vec<MovementAction>>;

pub fn create_movement_action_map(char: &Character) -> MovementActionMap {
    let mut movement_action_map = MovementActionMap::new();

    for movement_action in &char.movement_action {
        movement_action_map
            .entry(movement_action.id.clone())
            .or_insert(Vec::new())
            .push(movement_action.clone());
    }

    movement_action_map
}

pub fn load_character(
    char: &Character,
    assets: AssetMap,
    mut commands: &mut Commands,
    server: &Res<AssetServer>,
    ratio: Vec3,
    player_id: u64,
    game_settings: &Res<GameSettings>,
    spawn_transform: Transform,
    mut meshes: &mut ResMut<Assets<Mesh>>,
    mut materials: &mut ResMut<Assets<ColorMaterial>>,
    font: &Res<AugmentedFonts>,
    map: &Map,
) -> CharEntities {
    let rescale_ratio = Vec3::new(ratio.y, ratio.y, 1.0);

    println!("getting colliders for char");
    let mut colliders = char.collider.get_hitbox_bundles(ratio.y);

    let healthbar = create_healthbar(
        &char,
        &mut commands,
        rescale_ratio,
        &mut meshes,
        &mut materials,
        &map,
        player_id,
    );

    let attack_text =
        create_player_atk_text(&char, &mut commands, rescale_ratio, player_id, &font, &map);

    let player_id_text =
        create_player_id_text(&char, &mut commands, rescale_ratio, player_id, &font, &map);

    println!("finished getting colliders for char");
    let mut phys_map = create_physics_map(&char.physics_object, &rescale_ratio);
    let mut projectile_map = create_projectile_map(&char.projectile);
    let mut movement_map = create_movement_action_map(&char);
    let mut action_map = create_action_map(&char);
    load_char_components(
        &mut phys_map,
        &rescale_ratio,
        &assets,
        &mut commands,
        &char,
        player_id,
    );

    let core_phys = spawn_core(
        &char,
        &rescale_ratio,
        player_id,
        &assets,
        &mut commands,
        &phys_map,
        &mut colliders,
        spawn_transform,
        vec![
            (healthbar, SyncColliderFlags { rotation: false }),
            (player_id_text, SyncColliderFlags { rotation: false }),
            (attack_text, SyncColliderFlags { rotation: false }),
        ],
    );

    CharEntities {
        core: core_phys,
        static_component: None,
        actions: char.char_movement_action.scale_to_ratio(&ratio),
        attacks: char.attack.clone(),
        projectiles: projectile_map,
        attack_actions: action_map,
        rescale_ratio: ratio,
        physics_map: phys_map,
        collider_map: colliders,
        asset_map: assets,
        player_id,
        movement_map,
    }
}

impl Character {
    pub fn add_projectile_spawn_action_to_attack_map(&self, attack_map: &mut AttackMap) {
        for projectile_spawn_action in &self.spawn_projectile_action {
            if projectile_spawn_action.id != String::default() {
                let action_vec = attack_map
                    .entry(projectile_spawn_action.id.clone())
                    .or_insert(Vec::new());
                action_vec.push(AttackActions::SpawnProjectile(
                    projectile_spawn_action.clone(),
                ));
            }
        }
    }
    pub fn add_movement_attack_action_to_attack_map(&self, attack_map: &mut AttackMap) {
        for movement_action in &self.movement_attack_action {
            if movement_action.id != String::default() {
                let action_vec = attack_map
                    .entry(movement_action.id.clone())
                    .or_insert(Vec::new());
                action_vec.push(AttackActions::MoveAttackAction(movement_action.clone()));
            }
        }
    }
}

pub fn create_action_map(char: &Character) -> AttackMap {
    let mut action_map = AttackMap::new();

    char.add_projectile_spawn_action_to_attack_map(&mut action_map);
    char.add_movement_attack_action_to_attack_map(&mut action_map);

    action_map
}

#[derive(Debug, Clone, Inspectable, Default, Component)]
pub struct SyncTransformOffset {
    pub transform: Transform,
}

pub fn sync_objects_colliders(
    mut colliders: Query<(&Transform, &mut ColliderSyncEntity)>,
    mut sync_entities: Query<&mut Transform, (Without<ColliderSyncEntity>)>,
    sync_offest_transform: Query<&mut SyncTransformOffset>,
    asset_info: Res<AssetInfoMap>,
    svg_handle: Query<&Handle<Svg>>,
) {
    for (collider_position, mut entity_sync) in colliders.iter_mut() {
        let mut transform = collider_position.translation.xy();
        for (synced_entity, sync_flags) in entity_sync.synced_objects.iter_mut() {
            let entity = sync_entities.get_mut(*synced_entity);
            match entity {
                Ok(mut entity_transform) => {
                    entity_transform.translation = transform.extend(entity_transform.translation.z);

                    if sync_flags.rotation {
                        entity_transform.rotation = collider_position.rotation;
                    }

                    match sync_offest_transform.get(*synced_entity) {
                        Ok(sync_offset) => {
                            entity_transform.translation += sync_offset.transform.translation;
                            entity_transform
                                .rotation
                                .add(sync_offset.transform.rotation);
                        }
                        _ => {}
                    }

                    match svg_handle.get(*synced_entity) {
                        Ok(svg_handle) => match asset_info.get(&svg_handle.id) {
                            None => {}
                            Some(asset_info) => match asset_info {
                                AssetInfoType::Svg(svg_info) => {
                                    update_transform_svg(&svg_info, &mut entity_transform);
                                }
                                AssetInfoType::Image(_) => {}
                            },
                        },
                        Err(_) => {}
                    }
                }
                Err(error) => {
                    error!("Failed to sync collider to entity: {:?}", error)
                }
            }
        }
    }
}

pub fn update_dir_look(
    mut char_map: ResMut<CharComponentMap>,
    mut char_look: Query<(
        &MoveDirLookIdentifier,
        &CharComponentPlayerIdentifier,
        &mut SyncTransformOffset,
    )>,
    mut player_look_identifier: Query<&AAPlayerDescriptor>,
) {
    for (move_dir_id, player_id, mut sync_offset) in char_look.iter_mut() {
        match char_map.get(&player_id.player_id) {
            None => {}
            Some(character) => {
                let player_desc = match player_look_identifier.get(character.core) {
                    Ok(player_descriptor) => player_descriptor,
                    Err(_) => {
                        println!("Got core with no player descriptor in update dir look??");
                        return;
                    }
                };

                let dir_flags = player_desc.direction_facing.get_full_dir_flags();
                let base_rot_vector = Vec2::new(move_dir_id.move_dir_radius, 0.0);
                if dir_flags.contains(FullDirectionFacingFlags::NONE) {
                    sync_offset.transform.translation = Vec3::ZERO;
                    continue;
                };

                sync_offset.transform.translation =
                    Mat2::from_angle(dir_flags.rotation_angle(move_dir_id.dir_angles))
                        .mul_vec2(base_rot_vector)
                        .extend(0.0);
            }
        }
    }
}

pub fn healthbar_update(
    game: Res<Game>,
    health_query: Query<&mut PlayerHealth>,
    mut char_map: ResMut<CharComponentMap>,
    mut healthbar_query: Query<(
        &PlayerHealthBarId,
        &mut Mesh2dHandle,
        &mut Handle<ColorMaterial>,
    )>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut colour_material_assets: ResMut<Assets<ColorMaterial>>,
) {
    for (healthbar, mesh_handle, colour_material) in healthbar_query.iter_mut() {
        match char_map.get(&healthbar.player_id) {
            None => {}
            Some(character_entity) => match health_query.get(character_entity.core) {
                Ok(player_health) => {
                    let health_ratio =
                        (player_health.current_health / player_health.maximum_health);
                    match colour_material_assets.get_mut(colour_material.id) {
                        None => {}
                        Some(colour_material_asset) => {
                            let colour_addition =
                                game.selected_map.char_element_colours.health_colour_distr
                                    * (1.0 - health_ratio);
                            let colour =
                                Vec3::from(game.selected_map.char_element_colours.healthbar_max)
                                    + colour_addition;
                            colour_material_asset.color = Color::from((colour / 255.0).extend(1.0))
                        }
                    }

                    // Things to make it smoother in transitioning to 0 current health
                    let radius = if player_health.radius > player_health.width * health_ratio {
                        0.001
                    } else if player_health.width == 0.0 {
                        0.0
                    } else {
                        player_health.radius
                    };

                    match meshes.get_mut(mesh_handle.0.id) {
                        None => {}
                        Some(mesh_asset) => {
                            *mesh_asset = Mesh::from(RoundedBox {
                                size: Vec3::new(
                                    player_health.width / character_entity.rescale_ratio.y
                                        * health_ratio,
                                    player_health.height / character_entity.rescale_ratio.y,
                                    1.,
                                ),
                                radius: radius / character_entity.rescale_ratio.y,
                                subdivisions: player_health.subdivisions,
                            });
                        }
                    }
                }
                Err(_) => {
                    warn!(
                        "No health component for player {:#?} found",
                        character_entity.core
                    );
                }
            },
        }
    }
}
