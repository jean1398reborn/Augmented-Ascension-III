use std::collections::hash_map::Entry;
use std::fs::{DirEntry, ReadDir};
use std::path::PathBuf;

use anyhow::{Context, Result};
use bevy::asset::HandleId;
use bevy::log::error;
use bevy::math::Vec2Swizzles;
use bevy::prelude::*;
use bevy::tasks::AsyncComputeTaskPool;
use bevy::winit::WinitWindows;
use bevy_rapier2d::prelude::*;
use bevy_rapier2d::rapier::prelude::ColliderType;
use bevy_svg::prelude::Svg;
use semver::Version;
use serde::{Deserialize, Serialize};

use crate::assets::{
    get_asset, load_assets, Asset, AssetDirectory, AssetMap, AssetType, AssetVec, HandleIdVec,
    InterpolateHandles, TransmuteAsset,
};
use crate::background::create_bgs;
use crate::collider::*;
use crate::draw::get_info_scale_resolution;
use crate::universal::*;
use crate::{
    get_resolution, AppStates, AssetInfoMap, Game, RapierScaleConfig, WinitWindowsInfo, RAPIERSCALE,
};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Background {
    #[serde(default = "Vec::default")]
    pub bg_above: Vec<String>,

    pub bg_main: String,

    #[serde(default = "Vec::default")]
    pub bg_below: Vec<String>,
}

impl Default for Background {
    fn default() -> Self {
        Background {
            bg_above: Vec::default(),
            bg_main: String::default(),
            bg_below: Vec::default(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(default)]
pub struct SpawnPositions {
    pub positions: Vec<[f32; 2]>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SpriteAsset {
    pub asset: String,

    #[serde(default = "z_vecfault")]
    pub origin: [f32; 3],

    #[serde(default = "nz_vecfault")]
    pub scale: [f32; 3],

    #[serde(default = "f32::default")]
    pub rotation: f32,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(default)]
pub struct CharElementColours {
    pub healthbar_max: [f32; 3],
    pub healthbar_min: [f32; 3],
    pub player_id_text: [f32; 3],
    pub healthbar_underlay: [f32; 3],

    #[serde(skip)]
    pub health_colour_distr: Vec3,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Map {
    pub info: Info,

    #[serde(default = "Asset::default")]
    pub asset: Asset,

    #[serde(default = "Vec::default")]
    pub sprite: Vec<SpriteAsset>,

    #[serde(default = "PathBuf::default", skip_deserializing)]
    pub base_path: PathBuf,

    #[serde(default = "Background::default")]
    pub background: Background,

    #[serde(default = "AACollider::default")]
    pub collider: AACollider,

    #[serde(default = "MapUniversal::default")]
    pub universal: MapUniversal,

    #[serde(default)]
    pub spawn_positions: SpawnPositions,

    #[serde(default)]
    pub char_element_colours: CharElementColours,

    #[serde(default)]
    pub map_element_colours: MapElementColours,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, Copy)]
pub struct MapElementColours {
    pub big_centre_text_colour: [f32; 3],
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct MapUniversal {
    #[serde(default = "one_f32fault")]
    pub gravity_scale: f32,
}

impl PathAdjust for Map {
    fn change_path(&mut self, new_path: PathBuf) {
        self.base_path = new_path
    }
}

impl PossibleBundleRetrieve for SpriteAsset {
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

pub fn load_maps(mut commands: Commands, mut state: ResMut<State<AppStates>>) {
    let mut maps: Vec<Map> = vec![];
    println!("Loading maps...");
    load_directory("maps".into(), "main.toml", &mut maps);
    println!("Done loading maps...");
    state.set(AppStates::MainMenu);
    commands.insert_resource(maps);
}

impl TransmuteAsset for Map {
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

pub fn get_sprites(
    map: &Map,
    asset_server: &Res<AssetServer>,
    base_information: &Info,
    asset_map: &AssetMap,
    ratio: Vec3,
) -> Vec<PossibleBundle> {
    let mut sprites = vec![];

    for sprite in &map.sprite {
        sprites.push(sprite.retrieve_bundle(asset_map, ratio));
    }

    sprites
}

pub fn spawn_sprites(commands: &mut Commands, bundles: Vec<PossibleBundle>) {
    for bundle in bundles {
        match bundle {
            PossibleBundle::Sprite(spritebundle) => commands.spawn_bundle(spritebundle),
            PossibleBundle::Svg(svgbundle) => commands.spawn_bundle(svgbundle),
        }
        .insert(MapComponent);
    }
}

#[derive(Component)]
pub struct MapComponent;

pub fn load_map(
    mut game: ResMut<Game>,
    asset_server: Res<AssetServer>,
    mut commands: Commands,
    windows: Res<Windows>,
    winit_info: Res<WinitWindowsInfo>,
    asset_dir: Res<AssetDirectory>,
    mut rapier_config: ResMut<RapierScaleConfig>,
    mut rapier_gravity: ResMut<RapierConfiguration>,
    svg_assets: Res<Assets<Svg>>,
    window_descriptor: Res<WindowDescriptor>,
    mut state: ResMut<State<AppStates>>,
) {
    game.selected_map.char_element_colours.health_colour_distr =
        Vec3::from(game.selected_map.char_element_colours.healthbar_min)
            - Vec3::from(game.selected_map.char_element_colours.healthbar_max);
    let map = game.selected_map.clone();
    let asset_map = asset_dir.map_assets.clone();
    let resolution = get_resolution(windows);
    let rescale_resolution = (Vec2::from(map.info.base_dimensions) / resolution).y;

    // Since we cant modify the entire scale of the engine anymore post 0.12.0 or until the author updates it
    // we have to modify the transform of all objects and the gravity itself.
    rapier_gravity.gravity = RapierConfiguration::default().gravity;
    rapier_gravity.gravity /= rescale_resolution;
    rapier_gravity.gravity *= map.universal.gravity_scale;

    let (texture, texture_over, texture_under) = get_bg_textures(&map, &asset_server, &asset_map);

    let screen_ratio = winit_info.screen_dim;
    create_bgs(
        &mut commands,
        resolution,
        winit_info,
        texture,
        texture_over,
        texture_under,
        svg_assets,
    );

    let sprites = get_sprites(
        &map,
        &asset_server,
        &map.info,
        &asset_map,
        Vec3::new(rescale_resolution, rescale_resolution, 1.0),
    );
    spawn_sprites(&mut commands, sprites);

    // update the scale so that the window scaling is accurate to the map (platforms & sprites & such are also scaled
    //off of the y axis in universal)
    rapier_config.scale *= screen_ratio.y;

    let mut collider_map = map.collider.get_hitbox_bundles(rescale_resolution);

    println!("spawning colliders for map");
    for (key, remaining_colliders) in collider_map {
        for remaining_collider in remaining_colliders {
            match remaining_collider.collider_type {
                AAColliderType::Solid => {
                    commands
                        .spawn_bundle(remaining_collider)
                        .insert(SolidColliderIdentifier {})
                        .insert(MapComponent);
                }
                AAColliderType::JumpReset => {
                    commands
                        .spawn_bundle(remaining_collider)
                        .insert(JumpResetColliderIdentifier {})
                        .insert(MapComponent)
                        .insert(Sensor(true));
                }
                AAColliderType::Death => {
                    commands
                        .spawn_bundle(remaining_collider)
                        .insert(DeathColliderIdentifier {})
                        .insert(MapComponent)
                        .insert(Sensor(true));
                }
            }
        }
    }
    println!("done spawning colliders for map");

    state.set(AppStates::LoadChar);
}

pub fn get_bg_textures(
    map: &Map,
    asset_server: &Res<AssetServer>,
    asset_map: &AssetMap,
) -> (AssetType, Vec<AssetType>, Vec<AssetType>) {
    let texture = get_asset(&map.background.bg_main, &asset_map);

    let mut texture_over = vec![];
    for image_over in &map.background.bg_above {
        texture_over.push(get_asset(image_over, &asset_map))
    }

    let mut texture_under = vec![];
    for image_under in &map.background.bg_below {
        let mut asset = get_asset(image_under, &asset_map);
        texture_under.push(get_asset(image_under, &asset_map))
    }

    (texture, texture_over, texture_under)
}
