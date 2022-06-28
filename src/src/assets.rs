use std::collections::HashMap;
use std::f32::consts::PI;
use std::fmt::Debug;
use std::num::NonZeroU8;
use std::path::PathBuf;
use std::time::Instant;

use bevy::asset::{HandleId, LoadState};
use bevy::math::{Mat2, Vec3Swizzles};
use bevy::prelude::*;
use bevy::reflect::TypeUuid;
use bevy::render::render_resource::{FilterMode, SamplerDescriptor};
use bevy::tasks::{AsyncComputeTaskPool, Task};
use bevy::utils::label::DynHash;
use bevy_svg::prelude::{Svg, Svg2dBundle};
use num::Signed;
use serde::{Deserialize, Serialize};

use crate::background::{Background, BackgroundResolution};
use crate::universal::*;
use crate::{AppStates, Game, GameSettings};


// Handles that i should update to use linear interpolation to smoothen them
pub struct InterpolateHandles {
    pub handles: Vec<Handle<Image>>,
}

// Assetmap for the different assets for characters, maps
pub type AssetMap = HashMap<String, AssetType>;

// Font resource that i can pass around to get the font without having to reload it through asset server
#[derive(Debug, Clone)]
pub struct AugmentedFonts {
    pub bold_font: Handle<Font>,
    pub regular_font: Handle<Font>,
}

// A possible imageasset using in characters and maps as a sprite
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ImageAsset {
    pub name: String,
    pub path: String,
	

    // Determines if it should use linear interpolation sample mode or not
    #[serde(default = "default_true")]
    pub interpolate: bool,
}

// Simple types to just shorten a bit of typing and make code a bit cleaner.
pub type AssetVec = Vec<AssetType>;
pub type HandleIdVec = Vec<AssetHandleIdType>;


#[derive(Clone, Debug)]
pub struct SvgInfo {
    pub dimensions: Vec2,
}

#[derive(Clone, Debug)]
pub struct ImageInfo {}

#[derive(Clone, Debug)]
pub enum AssetInfoType {
    Svg(SvgInfo),
    Image(ImageInfo),
}

pub type AssetInfoMap = HashMap<HandleId, AssetInfoType>;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SvgAsset {
    pub name: String,
    pub path: String,
}

// This trait basically converts one of the file assets featured in map or character into usuable handles for spawning in game
pub trait TransmuteAsset {
    fn transmute_assets(
        &self,
        asset_server: &Res<AssetServer>,
        interpolate_handles: &mut ResMut<InterpolateHandles>,
    ) -> (AssetMap, HandleIdVec);
}


// Add svg assets to the asset map so we can spawn them later on
pub fn add_svg_assets(
    assets: &Vec<SvgAsset>,
    base_path: &PathBuf,
    asset_map: &mut AssetMap,
    asset_server: &Res<AssetServer>,
    handleid_vec: &mut HandleIdVec,
) {
    for svg_asset in assets {
        let mut base_path = base_path.clone();
        base_path.push(svg_asset.path.clone());
		
	  // Static typing required to tell compiler/bevy what it should load into
        let svg_handle: Handle<Svg> = asset_server.load(base_path);

        handleid_vec.push(AssetHandleIdType::Svg(svg_handle.id));
        asset_map.insert(svg_asset.name.clone(), AssetType::Svg(svg_handle));
    }
}

// Add image assets to the asset map for use in sprite2dbundles.
pub fn add_image_assets(
    assets: &Vec<ImageAsset>,
    base_path: &PathBuf,
    asset_map: &mut AssetMap,
    asset_server: &Res<AssetServer>,
    interpolate_handles: Option<&mut ResMut<InterpolateHandles>>,
    handleid_vec: &mut HandleIdVec,
) {
    let mut handles_to_interpolate = vec![];

    for image_asset in assets {
        let mut base_path = base_path.clone();
        base_path.push(image_asset.path.clone());

	  // Static typing required to tell compiler/bevy what it should load into
        let image_handle: Handle<Image> = asset_server.load(base_path);
	  
	  // Check if it should use interpolate filter or not which smoothens it
        if image_asset.interpolate {
            handles_to_interpolate.push(image_handle.clone());
        }

        handleid_vec.push(AssetHandleIdType::Image(image_handle.id));
        asset_map.insert(image_asset.name.clone(), AssetType::Image(image_handle));
    }
    
    // Add it to the list of things to interpolate
    match interpolate_handles {
        None => {}
        Some(interpolate_handles) => interpolate_handles
            .handles
            .append(&mut handles_to_interpolate),
    }
}


// Get an asset handle from the asset map for a certain type etc. Map or Character
pub fn get_asset(asset_id: &str, asset_map: &AssetMap) -> AssetType {
    match asset_map.get(asset_id) {
        Some(image) => image.clone(),
        _ => {
            println!("Invalid Asset: {}", asset_id);
            asset_map
                .get("missing_texture_icon_temporary")
                .unwrap()
                .clone()
        }
    }
}


// Updates the image sampler of assets so that it will experience linear interpolation
pub fn update_image_sampler(
    mut images: ResMut<Assets<Image>>,
    mut interpolate_list: ResMut<InterpolateHandles>,
) {
    // change the filtermode to use linear interpolation,
    // blurs some stuff to make it look less pixely

    interpolate_list.handles.retain(|image_handle| {
        if let Some(image) = images.get_mut(image_handle) {
            image.sampler_descriptor = SamplerDescriptor {
                mag_filter: FilterMode::Linear,
                min_filter: FilterMode::Linear,
                mipmap_filter: FilterMode::Linear,
                lod_min_clamp: 0.0,
                lod_max_clamp: 0.0,
                anisotropy_clamp: Some(NonZeroU8::new(16).unwrap()),
                ..image.sampler_descriptor
            };

            false
        } else {
            true
        }
    });
}


// Possible asset types
#[derive(Clone, Debug)]
pub enum AssetType {
    Image(Handle<Image>),
    Svg(Handle<Svg>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum AssetHandleIdType {
    Image(HandleId),
    Svg(HandleId),
}

// Load all assets 
pub fn load_assets(
    asset: &Asset,
    base_path: PathBuf,
    asset_server: &Res<AssetServer>,
    interpolate_handles: Option<&mut ResMut<InterpolateHandles>>,
) -> (AssetMap, HandleIdVec) {
    let mut asset_map = AssetMap::new();

    let mut handleid_vec = vec![];
    let temp = AssetType::Image(asset_server.load("assets/branding/icon.png"));
    asset_map.insert("missing_texture_icon_temporary".into(), temp);

    add_image_assets(
        &asset.image,
        &base_path,
        &mut asset_map,
        asset_server,
        interpolate_handles,
        &mut handleid_vec,
    );
    add_svg_assets(
        &asset.svg,
        &base_path,
        &mut asset_map,
        asset_server,
        &mut handleid_vec,
    );

    (asset_map, handleid_vec)
}

#[derive(Clone, Debug)]
pub struct UnloadedAssets {
    pub unloaded: HandleIdVec,
    pub loaded: HandleIdVec,
    pub origin_length: usize,
}

#[derive(Clone, Debug)]
pub struct AssetDirectory {
    pub char_assets: HashMap<u64, AssetMap>,
    pub map_assets: AssetMap,
}


// Struct for map and char which serves as a unified struct for both image and svgs so we can load them
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Asset {
    #[serde(default = "Vec::default")]
    pub image: Vec<ImageAsset>,

    #[serde(default = "Vec::default")]
    pub svg: Vec<SvgAsset>,
}

impl Default for Asset {
    fn default() -> Self {
        Asset {
            image: Vec::default(),
            svg: Vec::default(),
        }
    }
}

// Gets all the assets for the selected characters and maps to prepare them for use later on
pub fn retrieve_asset_maps(
    mut commands: Commands,
    game: Res<Game>,
    mut state: ResMut<State<AppStates>>,
    asset_server: Res<AssetServer>,
    svg_assets: Res<Assets<Svg>>,
    mut asset_information: ResMut<AssetInfoMap>,
    mut interpolate_handles: ResMut<InterpolateHandles>,
) {
    let (map_assets, unloaded_map_assets) = game
        .selected_map
        .transmute_assets(&asset_server, &mut interpolate_handles);

    let mut unloaded_assets = unloaded_map_assets;
    let mut char_assets = HashMap::new();

    for (index, character) in &game.selected_characters {
        let (char_asset, mut unloaded_char_assets) =
            character.transmute_assets(&asset_server, &mut interpolate_handles);
        unloaded_assets.append(&mut unloaded_char_assets);
        char_assets.insert(*index, char_asset);
    }

	
    // Stores the assets in an AssetDirectory type resource
    commands.insert_resource(AssetDirectory {
        char_assets,
        map_assets,
    });

    state.set(AppStates::LoadMap);
}

// Update the transform of svgs so its alignment is the same as with images.
pub fn update_svg_transforms(
    svg_assets: Res<Assets<Svg>>,
    mut commands: Commands,
    mut asset_information: ResMut<AssetInfoMap>,
    mut query_update: Query<(&Handle<Svg>, &mut UpdatedTransformComponent, &mut Transform)>,
) {
    for (svg_handle, mut update_transform, mut transform) in query_update.iter_mut() {

	  // Get the svg size so we can determine how much to move it by, also save the information in asset_informations so we can reuse it without having to get it again
        if update_transform.updated == false {
            let svg_info = match asset_information.get(&svg_handle.id) {
                None => {
                    let asset = match svg_assets.get(svg_handle.id) {
                        None => {
                            continue;
                        }
                        Some(asset) => asset,
                    };

                    let svg_info = AssetInfoType::Svg(SvgInfo {
                        dimensions: asset.size,
                    });

                    asset_information.insert(svg_handle.id, svg_info.clone());
                    svg_info
                }

                Some(asset_info) => asset_info.clone(),
            };

            let transform_adjust = match svg_info {
                AssetInfoType::Svg(svg_info) => update_transform_svg(&svg_info, &mut transform),
                _ => {
                    continue;
                }
            };

            update_transform.updated = true;
        }
    }
}

// Add all our fonts for usage.
pub fn add_augmented_fonts(
    mut commands: Commands,
    asset: Res<AssetServer>,
    settings: Res<GameSettings>,
) {
    let bold_font = asset.load(&settings.font_settings.bold_path);
    let regular_font = asset.load(&settings.font_settings.reg_path);

    commands.insert_resource(AugmentedFonts {
        bold_font,
        regular_font,
    });
}

// Small function which serves as our update function for the actual transforms
pub fn update_transform_svg(svg_info: &SvgInfo, transform: &mut Transform) {
    let mut centre_offset_dims = svg_info.dimensions * 0.5;
    centre_offset_dims.y *= -1.0;

    let mut transform_adjust = centre_offset_dims * transform.scale.xy();

    // Ouch math, basically finds how far away its current position is away from where it should be and moves it based on that so its at where it should be.
    let mut z_rot = transform.rotation.to_scaled_axis().z;
    let rotated_vector = Mat2::from_angle(z_rot)
        .mul_vec2(transform_adjust)
        .extend(0.0);
    transform.translation -= rotated_vector;
}
