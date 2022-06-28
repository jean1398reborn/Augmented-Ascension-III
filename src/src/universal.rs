use std::collections::HashMap;
use std::fs::{DirEntry, ReadDir};
use std::num::NonZeroU8;
use std::path::PathBuf;
use std::sync::Arc;

use bevy::asset::LoadState;
use bevy::math::{Vec2Swizzles, Vec3Swizzles};
use bevy::prelude::*;
use bevy::render::render_resource::{FilterMode, SamplerDescriptor};
use bevy::tasks::AsyncComputeTaskPool;
use bevy_inspector_egui::Inspectable;
use bevy_svg::prelude::*;
use semver::Version;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::assets::{get_asset, AssetInfoType, AssetMap, AssetType};
use crate::draw::get_info_scale_resolution;
use crate::{error, AssetInfoMap};

// The sync map
pub type SyncMap = Arc<RwLock<HashMap<usize, bool>>>;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Info {
    pub display_name: String,
    pub author: Option<String>,
    pub version: Option<Version>,
    pub description: Option<String>,
    pub keywords: Option<Vec<String>>,
    pub base_dimensions: [f32; 2],

    #[serde(default)]
    pub icon: String,
}

pub enum PossibleBundle {
    Sprite(SpriteBundle),
    Svg(AASvg2dBundle),
}

pub fn read_game_dir(dir: String) -> ReadDir {
    match std::fs::read_dir(&dir) {
        Ok(game_dir) => game_dir,
        Err(angry) => {
            error!("{}", angry);
            std::fs::create_dir(&dir).unwrap();
            std::fs::read_dir(&dir).unwrap()
        }
    }
}

pub fn load_item<T: serde::de::DeserializeOwned + PathAdjust>(
    possible_map: std::io::Result<DirEntry>,
    main_file: &str,
) -> Result<T, anyhow::Error> {
    let item = possible_map?;

    let mut path = item.path();
    path.push(main_file);

    let content = std::fs::read_to_string(path)?;
    let mut data: T = toml::from_str(&content).unwrap();

    data.change_path(item.path());
    Ok(data)
}

pub fn load_directory<T: serde::de::DeserializeOwned + PathAdjust>(
    dir: String,
    main_file: &str,
    load_type: &mut Vec<T>,
) {
    let read_dir = read_game_dir(dir);

    for possible_item in read_dir {
        load_type.push(match load_item(possible_item, main_file) {
            Ok(item) => item,
            Err(angry) => {
                error!("{}", angry);
                continue;
            }
        });
    }
}

pub trait PathAdjust {
    fn change_path(&mut self, new_path: PathBuf) {}
}

pub trait PossibleBundleRetrieve {
    fn retrieve_bundle(&self, assets: &AssetMap, ratio: Vec3) -> PossibleBundle;
}

pub trait PossibleBundleRetrieveWithAsset {
    fn retrieve_bundle_with_asset(
        &self,
        assets: &AssetMap,
        ratio: Vec3,
        asset: String,
    ) -> PossibleBundle;
}

#[derive(Component, Debug, Inspectable)]
pub struct UpdatedTransformComponent {
    pub updated: bool,
}

#[derive(Bundle)]
pub struct AASvg2dBundle {
    pub updated_transform: UpdatedTransformComponent,
    #[bundle]
    pub svg_bundle: Svg2dBundle,
}

pub fn retrieve_possible_bundle(
    assets: &AssetMap,
    scale: [f32; 3],
    origin: [f32; 3],
    rotation: f32,
    asset_id: &str,
    ratio: Vec3,
) -> PossibleBundle {
    let asset = get_asset(asset_id, assets);
    let mut transform = Transform::from_scale(Vec3::from(scale))
        .with_rotation(Quat::from_rotation_z(rotation))
        .with_translation(Vec3::from(origin));

    // modify translation to fit any dimensions of screen with base dimensions
    transform.scale /= ratio;
    transform.translation /= ratio;

    match asset {
        AssetType::Svg(handle) => {
            let svg_bundle = Svg2dBundle {
                svg: handle,
                transform,
                origin: Origin::TopLeft,
                ..Default::default()
            };

            PossibleBundle::Svg(AASvg2dBundle {
                updated_transform: UpdatedTransformComponent { updated: false },
                svg_bundle,
            })
        }
        AssetType::Image(handle) => PossibleBundle::Sprite(SpriteBundle {
            transform,
            texture: handle,
            ..Default::default()
        }),
    }
}

pub fn nz_vecfault() -> [f32; 3] {
    [1.0, 1.0, 1.0]
}
pub fn z_vecfault() -> [f32; 3] {
    [0.0, 0.0, 0.0]
}
pub fn one_u32fault() -> u32 {
    1
}
pub fn one_f32fault() -> f32 {
    1.0
}
pub fn default_true() -> bool {
    true
}
