use crate::assets::AssetMap;
use crate::char::{AttackBuffer, AttackKey, MovementAction};
use crate::collider::ColliderMap;
use crate::rigidbody::PhysicsSpawnExtras::SpawnTransform;
use crate::rigidbody::{AASyncSpawn, PhysMap, PhysicsSpawnExtras};
use crate::universal::*;
use crate::{
    AAPlayerDescriptor, AttackIdentifierTextId, AttackInstanceDirectory, CharComponentMap,
    CharEntities, ColliderSyncEntity, DirRotateAngles, FullDirectionFacingFlags, PlayerHealth,
    PlayerIdentifier, QueryEntityError, SyncColliderFlags, SyncTransformOffset, Vec3Swizzles,
};
use bevy::math::Mat2;
use bevy::prelude::*;
use bevy_inspector_egui::Inspectable;
use bevy_rapier2d::prelude::{Ccd, Damping, ExternalForce, GravityScale, Sensor, Velocity};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(default)]
pub struct Projectile {
    pub id: String,
    pub asset: String,

    #[serde(default = "f32::default")]
    pub rotation: f32,

    #[serde(default = "nz_vecfault")]
    pub scale: [f32; 3],

    pub spawn_z_axis: f32,

    pub spawn_origin_looking_radius: f32,

    #[serde(default = "z_vecfault")]
    pub spawn_origin_rel_abs: [f32; 3],

    #[serde(default = "z_vecfault")]
    pub sync_offset: [f32; 3],

    pub dont_phase_through: bool,
    pub damage: f32,
    pub pierce: u32,
    pub lifetime: f64,
    pub obey_gravity: bool,

    #[serde(default = "String::default")]
    pub physobj_id: String,

    #[serde(default)]
    pub face_dir_looking_angles: DirRotateAngles,
}
#[derive(Serialize, Deserialize, Debug, Clone, Component, Default)]
#[serde(default)]
pub struct Attack {
    pub actions: Vec<String>,
    pub cooldown: f64,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(default)]
pub struct SpawnProjectileAction {
    pub id: String,
    pub none_id: String,
    pub left_id: String,
    pub right_id: String,
    pub down_id: String,
    pub up_id: String,
    pub up_left_id: String,
    pub up_right_id: String,
    pub down_left_id: String,
    pub down_right_id: String,
    pub instance_id: String,
}
#[derive(Debug, Clone, Component, Inspectable)]
pub struct ProjectileIdentifier {
    pub created_timestamp: f64,
    pub lifetime: f64,
    pub damage: f32,
    pub pierce: u32,
    pub parent: Entity,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(default)]
pub struct MovementAttackAction {
    pub id: String,
    pub none_id: String,
    pub left_id: String,
    pub right_id: String,
    pub down_id: String,
    pub up_id: String,
    pub up_left_id: String,
    pub up_right_id: String,
    pub down_left_id: String,
    pub down_right_id: String,
    pub instance_id: String,
}

#[derive(Debug, Clone)]
pub enum AttackActions {
    SpawnProjectile(SpawnProjectileAction),
    MoveAttackAction(MovementAttackAction),
}

#[derive(Eq, Hash, PartialEq, Debug)]
pub enum InstanceIdDir {
    UpLeft(String),
    UpRight(String),
    DownRight(String),
    DownLeft(String),
    Left(String),
    Right(String),
    Down(String),
    Up(String),
    None(String),
}

pub enum InstanceItems {
    Projectile(Entity),
}

pub type AttackMap = HashMap<String, Vec<AttackActions>>;
pub type InstanceMap = HashMap<InstanceIdDir, Vec<InstanceItems>>;

impl Projectile {
    pub fn spawn_projectile(
        &self,
        dir_facing: &FullDirectionFacingFlags,
        char_entities: &CharEntities,
        player_descriptor: &mut AAPlayerDescriptor,
        transform: &mut Transform,
        commands: &mut Commands,
        instance: &mut InstanceMap,
        time: &Res<Time>,
    ) -> Option<Entity> {
        let mut extra_phys = vec![];

        if !self.dont_phase_through {
            extra_phys.push(PhysicsSpawnExtras::Sensor(Sensor(true)));
        }

        if !self.obey_gravity {
            extra_phys.push(PhysicsSpawnExtras::GravityScale(GravityScale(0.0)));
        }

        let mut spawn_transform = transform.clone();
        spawn_transform.rotation = Quat::from_rotation_z(self.rotation);
        spawn_transform.translation += Vec3::from(self.spawn_origin_rel_abs);

        let angle = dir_facing.rotation_angle(self.face_dir_looking_angles);
        let face_rot_vec = Vec2::new(self.spawn_origin_looking_radius, 0.);

        spawn_transform.translation += Mat2::from_angle(angle).mul_vec2(face_rot_vec).extend(0.0);

        let projectile_spawned = match self.retrieve_bundle(
            &char_entities.asset_map,
            spawn_transform.translation.xy(),
            char_entities.rescale_ratio,
        ) {
            PossibleBundle::Sprite(sprite) => commands.spawn_bundle(sprite),
            PossibleBundle::Svg(svg) => commands.spawn_bundle(svg),
        }
        .insert(SyncTransformOffset {
            transform: Transform::from_translation(Vec3::from(self.sync_offset)),
        })
        .id();

        println!("{:#?}", spawn_transform.translation);
        extra_phys.push(SpawnTransform(spawn_transform));
        extra_phys.push(PhysicsSpawnExtras::ProjectileIdentifier(
            ProjectileIdentifier {
                created_timestamp: time.seconds_since_startup(),
                lifetime: self.lifetime,
                damage: self.damage,
                pierce: self.pierce,
                parent: char_entities.core,
            },
        ));
        extra_phys.push(PhysicsSpawnExtras::ContinuousCollisionDetection(
            Ccd::enabled(),
        ));

        char_entities.physics_map.spawn_physics_object_with_sync(
            &self.physobj_id,
            vec![(projectile_spawned, SyncColliderFlags { rotation: true })],
            &char_entities.collider_map,
            commands,
            None,
            extra_phys,
            &char_entities.rescale_ratio,
        )
    }
}

impl FullDirectionFacingFlags {
    pub fn get_instance_id(&self, id: String) -> InstanceIdDir {
        match *self {
            FullDirectionFacingFlags::UP_LEFT => InstanceIdDir::UpLeft(id),
            FullDirectionFacingFlags::UP_RIGHT => InstanceIdDir::UpRight(id),
            FullDirectionFacingFlags::DOWN_RIGHT => InstanceIdDir::DownRight(id),
            FullDirectionFacingFlags::DOWN_LEFT => InstanceIdDir::DownLeft(id),
            FullDirectionFacingFlags::LEFT => InstanceIdDir::Left(id),
            FullDirectionFacingFlags::RIGHT => InstanceIdDir::Right(id),
            FullDirectionFacingFlags::DOWN => InstanceIdDir::Down(id),
            FullDirectionFacingFlags::UP => InstanceIdDir::Up(id),
            FullDirectionFacingFlags::NONE => InstanceIdDir::None(id),
            _ => InstanceIdDir::None(id),
        }
    }
}

impl SpawnProjectileAction {
    fn execute_action(
        &self,
        char_entities: &mut CharEntities,
        player_descriptor: &mut AAPlayerDescriptor,
        transform: &mut Transform,
        commands: &mut Commands,
        instance: &mut InstanceMap,
        time: &Res<Time>,
    ) {
        let dir_facing = player_descriptor.direction_facing.get_full_dir_flags();

        let projectile_id = match dir_facing {
            FullDirectionFacingFlags::UP_LEFT => &self.up_left_id,
            FullDirectionFacingFlags::UP_RIGHT => &self.up_right_id,
            FullDirectionFacingFlags::DOWN_LEFT => &self.down_left_id,
            FullDirectionFacingFlags::DOWN_RIGHT => &self.down_right_id,
            FullDirectionFacingFlags::UP => &self.up_id,
            FullDirectionFacingFlags::LEFT => &self.left_id,
            FullDirectionFacingFlags::DOWN => &self.down_id,
            FullDirectionFacingFlags::RIGHT => &self.right_id,
            FullDirectionFacingFlags::NONE => &self.none_id,
            _ => &self.none_id,
        }
        .clone();

        match char_entities.projectiles.get(&projectile_id) {
            None => {
                warn!("No Projectiles found under id {:#?}", projectile_id);
                return;
            }
            Some(projectiles) => {
                for projectile in projectiles {
                    let projectile_entity = projectile.spawn_projectile(
                        &dir_facing,
                        char_entities,
                        player_descriptor,
                        transform,
                        commands,
                        instance,
                        time,
                    );
                    match projectile_entity {
                        None => {}
                        Some(entity) => {
                            instance
                                .entry(dir_facing.get_instance_id(self.instance_id.clone()))
                                .or_insert(vec![])
                                .push(InstanceItems::Projectile(entity));
                        }
                    }
                }
            }
        }
    }
}

impl Projectile {
    pub fn retrieve_bundle(
        &self,
        assets: &AssetMap,
        spawn_position: Vec2,
        ratio: Vec3,
    ) -> PossibleBundle {
        retrieve_possible_bundle(
            assets,
            self.scale,
            spawn_position.extend(self.spawn_z_axis).to_array(),
            self.rotation,
            &self.asset,
            ratio,
        )
    }
}

pub fn attack_text_update(
    mut attack_text: Query<(&mut Text, &AttackIdentifierTextId)>,
    attack_buffer: Query<&AttackBuffer>,
    char_components: Res<CharComponentMap>,
) {
    for (mut text, identifier) in attack_text.iter_mut() {
        match char_components.get(&identifier.player_id) {
            None => {}
            Some(char_component) => match attack_buffer.get(char_component.core) {
                Ok(buffer) => {
                    let mut update_text = String::from("");

                    for key in &buffer.buffer {
                        match key {
                            &AttackKey::One => {
                                update_text.push_str("1");
                            }
                            &AttackKey::Two => {
                                update_text.push_str("2");
                            }
                        }
                    }

                    text.sections = vec![TextSection {
                        value: update_text,
                        style: identifier.text_style.clone(),
                    }];
                }
                Err(err) => {
                    warn!(
                        "Failed to get attack buffer when updating attack text: {:#?}",
                        err
                    )
                }
            },
        }
    }
}

impl MovementAttackAction {
    pub fn execute_action(
        &self,
        char_entities: &mut CharEntities,
        player_descriptor: &mut AAPlayerDescriptor,
        instance: &InstanceMap,
        projectiles: &mut Query<(
            &mut Velocity,
            &mut ExternalForce,
            &mut ProjectileIdentifier,
            &mut Damping,
        )>,
    ) {
        // yumm boilerplate yay
        let dir_facing = player_descriptor.direction_facing.get_full_dir_flags();

        let action_id = match dir_facing {
            FullDirectionFacingFlags::UP_LEFT => &self.up_left_id,
            FullDirectionFacingFlags::UP_RIGHT => &self.up_right_id,
            FullDirectionFacingFlags::DOWN_LEFT => &self.down_left_id,
            FullDirectionFacingFlags::DOWN_RIGHT => &self.down_right_id,
            FullDirectionFacingFlags::UP => &self.up_id,
            FullDirectionFacingFlags::LEFT => &self.left_id,
            FullDirectionFacingFlags::DOWN => &self.down_id,
            FullDirectionFacingFlags::RIGHT => &self.right_id,
            FullDirectionFacingFlags::NONE => &self.none_id,
            _ => &self.none_id,
        }
        .clone();

        let movement_action = match char_entities.movement_map.get(&action_id) {
            None => {
                warn!(
                    "No Movement actions for Attack Movement Action: {:#?}",
                    self.id
                );
                return;
            }
            Some(movement_actions) => movement_actions,
        };

        for action in movement_action {
            if action.instance_affected_id == String::default() {
                warn!("No instance id for Movement action: {:#?}", action.id);
            }
            let instance_dir_id = dir_facing.get_instance_id(action.instance_affected_id.clone());

            match instance.get(&instance_dir_id) {
                None => {}
                Some(instance_items) => {
                    for instance_item in instance_items {
                        match instance_item {
                            InstanceItems::Projectile(projectile) => {
                                let (
                                    mut velocity,
                                    mut external_force,
                                    mut projectile_identifier,
                                    mut damping,
                                ) = match projectiles.get_mut(*projectile) {
                                    Ok(projectile_muts) => projectile_muts,
                                    Err(err) => {
                                        warn!(
                                            "No projectile item found for id: {:#?}: {:#?}",
                                            instance_dir_id, err
                                        );
                                        return;
                                    }
                                };

                                action.add_values(&mut velocity, &mut external_force, &mut damping);

                                action.set_values(&mut velocity, &mut external_force, &mut damping);
                            }
                        }
                    }
                }
            }
        }
    }
}

pub struct UnusedAction {
    pub actions: Vec<AttackActions>,
    pub player_id: u64,
    pub instance_id: u64,
}

impl Attack {
    pub fn execute_attack(
        &self,
        char_entities: &mut CharEntities,
        player_descriptor: &mut AAPlayerDescriptor,
        transform: &mut Transform,
        commands: &mut Commands,
        attack_directory: &mut ResMut<AttackInstanceDirectory>,
        time: &Res<Time>,
    ) {
        let mut attack_instance = InstanceMap::new();
        let mut unexecuted_actions = vec![];

        for attack_action in &self.actions {
            match char_entities.attack_actions.get_mut(attack_action) {
                None => {}
                Some(actions) => {
                    for action in actions.clone() {
                        match action {
                            AttackActions::SpawnProjectile(projectile_action) => {
                                projectile_action.execute_action(
                                    char_entities,
                                    player_descriptor,
                                    transform,
                                    commands,
                                    &mut attack_instance,
                                    time,
                                );
                            }
                            AttackActions::MoveAttackAction(movement_attacktion) => {
                                unexecuted_actions
                                    .push(AttackActions::MoveAttackAction(movement_attacktion));
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        if unexecuted_actions.len() > 0 {
            let current_id = attack_directory.previous_used_id;
            attack_directory.previous_used_id += 1;

            let unused_attack = UnusedAction {
                actions: unexecuted_actions,
                player_id: char_entities.player_id,
                instance_id: current_id,
            };

            attack_directory
                .attack_instances
                .insert(current_id, attack_instance);
            attack_directory.unexecuted_actions.push(unused_attack);
        }
    }
}

pub trait ExecuteOptionAttack {
    fn execute_attack(
        &self,
        char_entities: &mut CharEntities,
        player_descriptor: &mut AAPlayerDescriptor,
        transform: &mut Transform,
        commands: &mut Commands,
        attack_directory: &mut ResMut<AttackInstanceDirectory>,
        time: &Res<Time>,
    );
}

impl ProjectileIdentifier {
    pub fn apply_projectile(
        &mut self,
        health: &mut Mut<PlayerHealth>,
        player_id: &mut Mut<PlayerIdentifier>,
        player_descriptor: &mut Mut<AAPlayerDescriptor>,
    ) {
        health.current_health -= self.damage;
        self.pierce -= 1;
    }
}

impl ExecuteOptionAttack for Option<Attack> {
    fn execute_attack(
        &self,
        char_entities: &mut CharEntities,
        player_descriptor: &mut AAPlayerDescriptor,
        transform: &mut Transform,
        commands: &mut Commands,
        attack_directory: &mut ResMut<AttackInstanceDirectory>,
        time: &Res<Time>,
    ) {
        match self {
            None => {}
            Some(attack) => {
                attack.execute_attack(
                    char_entities,
                    player_descriptor,
                    transform,
                    commands,
                    attack_directory,
                    time,
                );
            }
        }
    }
}

pub type ProjectileMap = HashMap<String, Vec<Projectile>>;

pub fn create_projectile_map(projectiles: &Vec<Projectile>) -> ProjectileMap {
    let mut projectile_map = ProjectileMap::new();

    for projectile in projectiles {
        let projectile_vec = projectile_map
            .entry(projectile.id.clone())
            .or_insert(vec![]);

        projectile_vec.push(projectile.clone());
    }

    projectile_map
}

pub fn execute_unused_actions(
    mut attack_directory: ResMut<AttackInstanceDirectory>,
    mut projectile_query: Query<(
        &mut Velocity,
        &mut ExternalForce,
        &mut ProjectileIdentifier,
        &mut Damping,
    )>,
    mut player_query: Query<(&mut AAPlayerDescriptor, &mut Transform)>,
    mut commands: Commands,
    mut char_map: ResMut<CharComponentMap>,
) {
    let mut remove_attack_instance = vec![];
    for unexecuted_action in &attack_directory.unexecuted_actions {
        let instance_map = match attack_directory
            .attack_instances
            .get(&unexecuted_action.instance_id)
        {
            None => {
                warn!(
                    "No instance Map found for id: {:#?}",
                    unexecuted_action.instance_id
                );
                continue;
            }
            Some(instance_map) => instance_map,
        };

        let char_entities = match char_map.get_mut(&unexecuted_action.player_id) {
            None => {
                warn!(
                    "No Player CharEntities found for player id {:#?}",
                    unexecuted_action.player_id
                );
                continue;
            }
            Some(char_entities) => char_entities,
        };

        let (mut player_descriptor, mut transform) = match player_query.get_mut(char_entities.core)
        {
            Err(err) => {
                warn!(
                    "No Player Descriptor found for player id {:#?}: {:#?}",
                    unexecuted_action.player_id, err
                );
                continue;
            }
            Ok(player_mut) => player_mut,
        };

        for action in &unexecuted_action.actions {
            match action {
                AttackActions::SpawnProjectile(projectile) => {}
                AttackActions::MoveAttackAction(move_action) => {
                    move_action.execute_action(
                        char_entities,
                        &mut player_descriptor,
                        instance_map,
                        &mut projectile_query,
                    );
                }
            }
        }

        remove_attack_instance.push(unexecuted_action.instance_id);
    }

    attack_directory
        .unexecuted_actions
        .retain(|unexecuted_action| {
            if remove_attack_instance.contains(&unexecuted_action.instance_id) {
                false
            } else {
                true
            }
        });

    for instance_id in remove_attack_instance {
        attack_directory.attack_instances.remove(&instance_id);
    }
}

pub fn projectile_lifetimes(
    mut projectile_query: Query<(Entity, &ProjectileIdentifier, &ColliderSyncEntity)>,
    mut commands: Commands,
    time: Res<Time>,
) {
    for (entity, projectile_id, collider_entity) in projectile_query.iter() {
        if projectile_id.created_timestamp < (time.seconds_since_startup() - projectile_id.lifetime)
        {
            collider_entity.despawn_self(&mut commands);
            commands.entity(entity).despawn_recursive();
        }
    }
}

impl ColliderSyncEntity {
    pub fn despawn_self(&self, commands: &mut Commands) {
        for (synced_entity, _collider_flags) in &self.synced_objects {
            commands.entity(*synced_entity).despawn_recursive();
        }
    }
}
