use std::collections::HashMap;
use std::ops::Index;

use crate::{AAPlayerDescriptor, CharacterInputSettings, SyncColliderFlags, VelocityForceCap};
use bevy::prelude::*;
use bevy_rapier2d::prelude::*;
use serde::{Deserialize, Serialize};

use crate::action_traits::ScaleToRatio;
use crate::char::{AttackBuffer, PlayerHealth, PlayerIdentifier};
use crate::collider::{
    AAColliderType, AugmentedCollider, ColliderMap, ColliderSyncEntity, DeathColliderIdentifier,
    JumpResetColliderIdentifier, SolidColliderIdentifier,
};
use crate::projectile::ProjectileIdentifier;
use crate::universal::*;

pub type PhysMap = HashMap<String, AAPhysicsObject>;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PhysicsObject {
    pub collider_ids: Vec<String>,
    pub id: String,

    //AARigidBody elements
    #[serde(default = "one_f32fault")]
    pub gravity_scale: f32,

    #[serde(default = "bool::default")]
    pub disable_rotation: bool,

    #[serde(default = "f32::default")]
    pub linear_damping: f32,

    #[serde(default = "f32::default")]
    pub angular_damping: f32,
}

pub struct AAPhysicsObject {
    pub rigid_body: AARigidBody,
    pub extra_synced_items: Vec<(Entity, SyncColliderFlags)>,
    pub colliders: Vec<String>,
}

#[derive(Bundle, Clone)]
pub struct AARigidBody {
    pub rigid_body: RigidBody,
    pub gravity_scale: GravityScale,
    pub axis_lock: LockedAxes,
    pub damping: Damping,
}

pub fn create_physics_map(physics_objects: &Vec<PhysicsObject>, rescale_ratio: &Vec3) -> PhysMap {
    let mut physmap = PhysMap::new();
    for phys_obj in physics_objects {
        // an option to lock the rotation axis for a physics collider which disables it rotating
        let locked_axes = match phys_obj.disable_rotation {
            true => LockedAxes::ROTATION_LOCKED,
            false => LockedAxes::empty(),
        };

        // The damping which is basically kind of like air resistance, scaled by the screen ratio since it should change
        let damping = Damping {
            linear_damping: phys_obj.linear_damping / rescale_ratio.y,
            angular_damping: phys_obj.angular_damping / rescale_ratio.y,
        };

        // The rigidbody in the form of a Bundle
        let rigid_body = AARigidBody {
            rigid_body: RigidBody::Dynamic,
            gravity_scale: GravityScale(phys_obj.gravity_scale),
            axis_lock: locked_axes,
            damping: damping,
        };

        let phys_id = phys_obj.id.clone();

        // Wrapped it inside of another struct to preserve the collider ids
        let phys_obj = AAPhysicsObject {
            colliders: phys_obj.collider_ids.clone(),
            rigid_body,
            extra_synced_items: vec![],
        };

        physmap.insert(phys_id, phys_obj);
    }
    physmap
}

pub enum PhysicsSpawnExtras {
    PlayerIdentifier(PlayerIdentifier),
    AAPlayerDescriptor(AAPlayerDescriptor),
    CharacterControls(CharacterInputSettings),
    VelocityCap(VelocityForceCap),
    SpawnTransform(Transform),
    PlayerHP(PlayerHealth),
    AttackBuffer(AttackBuffer),
    Sensor(Sensor),
    GravityScale(GravityScale),
    ContinuousCollisionDetection(Ccd),
    ProjectileIdentifier(ProjectileIdentifier),
}

pub trait AASyncSpawn {
    fn spawn_physics_object_with_sync(
        &self,
        phys_id: &String,
        sync_entities: Vec<(Entity, SyncColliderFlags)>,
        collider_map: &ColliderMap,
        commands: &mut Commands,
        joint: Option<ImpulseJoint>,
        extras: Vec<PhysicsSpawnExtras>,
        ratio: &Vec3,
    ) -> Option<Entity>;
}

pub enum ExtraInserts {
    SolidColliderIdentifier(SolidColliderIdentifier),
    JumpResetColliderIdentifier(JumpResetColliderIdentifier),
    DeathColliderIdentifier(DeathColliderIdentifier),
    Sensor(Sensor),
}

impl AASyncSpawn for PhysMap {
    fn spawn_physics_object_with_sync(
        &self,
        phys_id: &String,
        sync_entities: Vec<(Entity, SyncColliderFlags)>,
        collider_map: &ColliderMap,
        commands: &mut Commands,
        joint: Option<ImpulseJoint>,
        extras: Vec<PhysicsSpawnExtras>,
        ratio: &Vec3,
    ) -> Option<Entity> {
        match self.get(phys_id) {
            None => None,
            Some(physics_obj) => {
                let mut colliders = vec![];

                // Get all colliders listed by the physics object
                for collider_id in &physics_obj.colliders {
                    match collider_map.get(&Some(collider_id.clone())) {
                        None => {}
                        Some(new_colliders) => colliders.append(&mut new_colliders.clone()),
                    }
                }

                println!("{:#?}", colliders.len());

                // Spawn the physics object
                let mut phys_entity = commands.spawn_bundle(physics_obj.rigid_body.clone());

                match joint {
                    None => {}
                    Some(impulse_joint) => {
                        phys_entity.insert(impulse_joint);
                    }
                };

                let mut collider_sync = ColliderSyncEntity {
                    synced_objects: sync_entities,
                };

                for extra_synced_item in &physics_obj.extra_synced_items {
                    collider_sync.synced_objects.push(*extra_synced_item);
                }

                let mut collider_sensor = false;

                for extra in extras {
                    match extra {
                        PhysicsSpawnExtras::PlayerIdentifier(identifier) => {
                            phys_entity.insert(identifier);
                        }
                        PhysicsSpawnExtras::AAPlayerDescriptor(descriptor) => {
                            phys_entity.insert(descriptor);
                        }
                        PhysicsSpawnExtras::CharacterControls(controls) => {
                            phys_entity.insert(controls);
                        }
                        PhysicsSpawnExtras::VelocityCap(mut cap) => {
                            // Update it to make sure its scaled properly
                            cap = cap.scale_to_ratio(ratio);

                            phys_entity.insert(cap);
                        }
                        PhysicsSpawnExtras::SpawnTransform(spawn_transform) => {
                            phys_entity.insert(spawn_transform);
                        }
                        PhysicsSpawnExtras::PlayerHP(player_health) => {
                            phys_entity.insert(player_health);
                        }
                        PhysicsSpawnExtras::AttackBuffer(buffer) => {
                            phys_entity.insert(buffer);
                        }
                        PhysicsSpawnExtras::Sensor(sensor) => collider_sensor = true,
                        PhysicsSpawnExtras::ContinuousCollisionDetection(ccd) => {
                            phys_entity.insert(ccd);
                        }
                        PhysicsSpawnExtras::ProjectileIdentifier(projectile_identifier) => {
                            phys_entity.insert(projectile_identifier);
                        }
                        PhysicsSpawnExtras::GravityScale(scale) => {
                            phys_entity.insert(scale);
                        }
                        _ => {}
                    }
                }

                // Utilise parent/child relationship so that each collider is like sort of "meshed" together
                // through bevy_rapier so their properties and shape combine
                // Sync to the actual entity so that they look like they're attached together
                phys_entity
                    .with_children(|parent| {
                        for collider in colliders {
                            let mut extra_inserts = vec![];

                            match collider.collider_type {
                                AAColliderType::Solid => {
                                    extra_inserts.push(ExtraInserts::SolidColliderIdentifier(
                                        SolidColliderIdentifier {},
                                    ));
                                }
                                AAColliderType::JumpReset => {
                                    extra_inserts.push(ExtraInserts::JumpResetColliderIdentifier(
                                        JumpResetColliderIdentifier {},
                                    ));
                                    collider_sensor = true;
                                }
                                AAColliderType::Death => {
                                    extra_inserts.push(ExtraInserts::DeathColliderIdentifier(
                                        DeathColliderIdentifier {},
                                    ));
                                    collider_sensor = true;
                                }
                            };

                            if collider_sensor {
                                extra_inserts.push(ExtraInserts::Sensor(Sensor(true)));
                            }

                            let mut collider_commands = parent.spawn_bundle(collider);

                            for extra in extra_inserts {
                                match extra {
                                    ExtraInserts::SolidColliderIdentifier(identifier) => {
                                        collider_commands.insert(identifier);
                                    }
                                    ExtraInserts::JumpResetColliderIdentifier(identifier) => {
                                        collider_commands.insert(identifier);
                                    }
                                    ExtraInserts::DeathColliderIdentifier(identifier) => {
                                        collider_commands.insert(identifier);
                                    }
                                    ExtraInserts::Sensor(sensor) => {
                                        collider_commands.insert(sensor);
                                    }
                                }
                            }
                        }
                    })
                    .insert(Velocity::default())
                    .insert(ExternalForce::default());

                phys_entity.insert(collider_sync);

                Some(phys_entity.id())
            }
        }
    }
}
