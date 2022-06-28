use std::collections::HashMap;

use bevy::ecs::bundle::Bundle;
use bevy::ecs::component::Component;
use bevy::math::Vec3Swizzles;
use bevy::prelude::{Entity, Quat, Transform, Vec2, Vec3};
use bevy_inspector_egui::Inspectable;
use bevy_rapier2d::prelude::*;
use nalgebra::{Const, OPoint, Point2};
use serde::{Deserialize, Serialize};

use crate::universal::*;

pub type ColliderMap = HashMap<Option<String>, Vec<AugmentedCollider>>;

#[derive(Copy, Clone, Inspectable, Component, Default, Debug)]
pub struct SyncColliderFlags {
    pub rotation: bool,
}

#[derive(Clone, Bundle)]
pub struct AugmentedCollider {
    pub collider: Collider,
    pub transform: Transform,
    pub mass: ColliderMassProperties,
    pub friction: Friction,
    pub restitution: Restitution,
    pub active_event: ActiveEvents,
    pub collider_type: AAColliderType,
}

#[derive(Clone, Component, Copy)]
pub enum AAColliderType {
    Solid,
    JumpReset,
    Death,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(default)]
pub struct CollidersVec {
    colliders: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(default)]
pub struct AACollider {
    pub cuboid: Vec<CuboidCollider>,
    pub circle: Vec<CircleCollider>,
    pub convex: Vec<ConvexCollider>,
    pub round_cuboid: Vec<RoundCuboidCollider>,
    pub jump_reset_colliders: CollidersVec,
    pub death_colliders: CollidersVec,
}

impl Default for AACollider {
    fn default() -> Self {
        Self {
            cuboid: vec![],
            circle: vec![],
            convex: vec![],
            round_cuboid: vec![],
            jump_reset_colliders: CollidersVec { colliders: vec![] },
            death_colliders: CollidersVec { colliders: vec![] },
        }
    }
}
#[derive(Component)]
pub struct ColliderSyncEntity {
    pub synced_objects: Vec<(Entity, SyncColliderFlags)>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CuboidCollider {
    pub id: Option<String>,
    pub width: f32,
    pub height: f32,

    #[serde(default = "z_vecfault")]
    pub origin: [f32; 3], //kept it 3 values for consistency, but z axis for hitboxes is useless

    #[serde(default)]
    pub rotation: f32,

    #[serde(default = "one_f32fault")]
    pub density: f32,

    #[serde(default = "f32::default")]
    pub friction: f32,

    #[serde(default = "f32::default")]
    pub restitution: f32,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RoundCuboidCollider {
    pub id: Option<String>,
    pub width: f32,
    pub height: f32,
    pub border_radius: f32,

    #[serde(default = "z_vecfault")]
    pub origin: [f32; 3], //kept it 3 values for consistency, but z axis for hitboxes is useless

    #[serde(default)]
    pub rotation: f32,

    #[serde(default = "one_f32fault")]
    pub density: f32,

    #[serde(default = "f32::default")]
    pub friction: f32,

    #[serde(default = "f32::default")]
    pub restitution: f32,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CircleCollider {
    pub id: Option<String>,
    pub radius: f32,

    #[serde(default = "z_vecfault")]
    pub origin: [f32; 3],

    #[serde(default)]
    pub rotation: f32,

    #[serde(default = "one_f32fault")]
    pub density: f32,

    #[serde(default = "f32::default")]
    pub friction: f32,

    #[serde(default = "f32::default")]
    pub restitution: f32,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ConvexCollider {
    pub id: Option<String>,
    pub points: Vec<[f32; 2]>,
    pub indices: Vec<[u32; 2]>,

    #[serde(default = "z_vecfault")]
    pub origin: [f32; 3],

    #[serde(default)]
    pub rotation: f32,

    #[serde(default = "one_f32fault")]
    pub density: f32,

    #[serde(default = "f32::default")]
    pub friction: f32,

    #[serde(default = "f32::default")]
    pub restitution: f32,
}

pub trait GetColliderInfo {
    fn get_collider_shape(&self, rescale: f32) -> Collider;
    fn get_origin(&self) -> Vec3;
    fn get_id(&self) -> Option<String>;
    fn get_density(&self) -> f32;
    fn get_friction(&self) -> f32;
    fn get_restitution(&self) -> f32;
    fn get_rotation(&self) -> f32;
}

pub fn add_collider_map<T: GetColliderInfo>(colliders: &Vec<T>, scale: f32, map: &mut ColliderMap) {
    for collider in colliders {
        let current_id_collider_vec = map.entry(collider.get_id()).or_insert(vec![]);

        current_id_collider_vec.push(get_collider(collider, scale));
    }
}

pub fn get_collider<T: GetColliderInfo>(collider: &T, rescale: f32) -> AugmentedCollider {
    let shape = collider.get_collider_shape(rescale);

    // Transform collider position/rotation according to file
    let mut collider_transform =
        Transform::from_translation((collider.get_origin().xy() / rescale).extend(0.0))
            .with_rotation(Quat::from_rotation_z(collider.get_rotation()));

    let mut collider_bundle = AugmentedCollider {
        collider: shape.into(),
        transform: collider_transform,
        mass: ColliderMassProperties::Density(collider.get_density()),
        friction: Friction::coefficient(collider.get_friction()),
        restitution: Restitution::coefficient(collider.get_restitution()),
        active_event: ActiveEvents::COLLISION_EVENTS,
        collider_type: AAColliderType::Solid,
    };
    println!("{:#?}", (collider.get_origin().xy() / rescale));
    collider_bundle
}

impl GetColliderInfo for CuboidCollider {
    fn get_collider_shape(&self, rescale: f32) -> Collider {
        let width = (self.width / 2.0) / rescale;
        let height = (self.height / 2.0) / rescale;

        Collider::cuboid(width, height)
    }
    fn get_id(&self) -> Option<String> {
        self.id.clone()
    }
    fn get_origin(&self) -> Vec3 {
        Vec3::from(self.origin)
    }
    fn get_density(&self) -> f32 {
        self.density
    }
    fn get_friction(&self) -> f32 {
        self.friction
    }
    fn get_restitution(&self) -> f32 {
        self.restitution
    }
    fn get_rotation(&self) -> f32 {
        self.rotation
    }
}

impl GetColliderInfo for RoundCuboidCollider {
    fn get_collider_shape(&self, rescale: f32) -> Collider {
        let width = (self.width / 2.0) / rescale;
        let height = (self.height / 2.0) / rescale;
        let border_radius = self.border_radius / rescale;

        Collider::round_cuboid(width, height, border_radius)
    }
    fn get_id(&self) -> Option<String> {
        self.id.clone()
    }
    fn get_origin(&self) -> Vec3 {
        Vec3::from(self.origin)
    }
    fn get_density(&self) -> f32 {
        self.density
    }
    fn get_friction(&self) -> f32 {
        self.friction
    }
    fn get_restitution(&self) -> f32 {
        self.restitution
    }
    fn get_rotation(&self) -> f32 {
        self.rotation
    }
}

impl GetColliderInfo for CircleCollider {
    fn get_collider_shape(&self, rescale: f32) -> Collider {
        Collider::ball(self.radius / rescale)
    }

    fn get_id(&self) -> Option<String> {
        self.id.clone()
    }
    fn get_origin(&self) -> Vec3 {
        Vec3::from(self.origin)
    }
    fn get_density(&self) -> f32 {
        self.density
    }
    fn get_friction(&self) -> f32 {
        self.friction
    }
    fn get_restitution(&self) -> f32 {
        self.restitution
    }
    fn get_rotation(&self) -> f32 {
        self.rotation
    }
}

impl GetColliderInfo for ConvexCollider {
    fn get_collider_shape(&self, rescale: f32) -> Collider {
        let points = self
            .points
            .iter()
            .map(|p| Vec2::new(p[0], p[1]) / rescale)
            .collect::<Vec<_>>();

        Collider::convex_decomposition(points.as_slice(), self.indices.as_slice())
    }
    fn get_id(&self) -> Option<String> {
        self.id.clone()
    }
    fn get_origin(&self) -> Vec3 {
        Vec3::from(self.origin)
    }
    fn get_density(&self) -> f32 {
        self.density
    }
    fn get_friction(&self) -> f32 {
        self.friction
    }
    fn get_restitution(&self) -> f32 {
        self.restitution
    }
    fn get_rotation(&self) -> f32 {
        self.rotation
    }
}

#[derive(Component)]
pub struct SolidColliderIdentifier {}

#[derive(Component)]
pub struct DeathColliderIdentifier {}

#[derive(Component)]
pub struct JumpResetColliderIdentifier {}

impl AACollider {
    pub fn get_hitbox_bundles(&self, size: f32) -> ColliderMap {
        let mut bundles = ColliderMap::new();

        add_collider_map(&self.cuboid, size, &mut bundles);
        add_collider_map(&self.circle, size, &mut bundles);
        add_collider_map(&self.convex, size, &mut bundles);
        add_collider_map(&self.round_cuboid, size, &mut bundles);

        println!("adding jump reset stuff");
        // Check for jump reset colliders
        self.jump_reset_colliders
            .set_special_type(&mut bundles, AAColliderType::JumpReset);
        self.death_colliders
            .set_special_type(&mut bundles, AAColliderType::Death);
        println!("finished adding juimp reset stuff");
        bundles
    }
}

impl CollidersVec {
    pub fn set_special_type(&self, bundles: &mut ColliderMap, collider_type: AAColliderType) {
        for id in &self.colliders {
            match bundles.get_mut(&Some(id.clone())) {
                None => {}
                Some(collider_vec) => {
                    for collider in collider_vec {
                        collider.collider_type = collider_type;
                        collider.mass = ColliderMassProperties::Density(0.0);
                    }
                }
            }
        }
    }
}
