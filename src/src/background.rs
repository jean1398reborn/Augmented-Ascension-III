use bevy::prelude::*;
use bevy::tasks::AsyncComputeTaskPool;
use bevy::window::WindowId;
use bevy::winit::WinitWindows;
use bevy_svg::prelude::{Origin, Svg, Svg2dBundle};
use num;
use winit::dpi::PhysicalSize;

use crate::assets::AssetType;
use crate::draw::*;
use crate::{MapComponent, UpdatedTransformComponent};

#[derive(Component)]
pub struct Background {
    pub processed: bool,
}

pub struct BackgroundResolution {
    pub resolution: Vec2,
}

pub fn create_bgs(
    commands: &mut Commands,
    mut resolution: Vec2,
    winit_info: Res<WinitWindowsInfo>,
    texture: AssetType,
    texture_over: Vec<AssetType>,
    texture_under: Vec<AssetType>,
    svg_assets: Res<Assets<Svg>>,
) {
    let background_sprite = Sprite {
        custom_size: Some(resolution),
        ..Default::default()
    };

    commands.insert_resource(BackgroundResolution { resolution });

    let size = winit_info.screen_dim;

    let ratio_x = ((size.x as f32) / resolution.x).ceil();
    let ratio_y = ((size.y as f32) / resolution.y).ceil();

    spawn_bg(
        commands,
        background_sprite,
        texture,
        texture_over,
        texture_under,
        ratio_x as u32,
        ratio_y as u32,
        resolution,
        svg_assets,
    );
}

pub fn spawn_bg(
    commands: &mut Commands,
    sprite: Sprite,
    texture: AssetType,
    texture_over: Vec<AssetType>,
    texture_under: Vec<AssetType>,
    amount_spawned_x: u32,
    amount_spawned_y: u32,
    resolution: Vec2,
    svg_assets: Res<Assets<Svg>>,
) {
    for iteration_x in 0..amount_spawned_x {
        let mut transforms = calculate_transforms_x(iteration_x, resolution, texture.clone());
        let mut total_transforms = vec![];

        for iteration_y in 1..amount_spawned_y {
            calculate_transforms_y(
                &mut transforms,
                &mut total_transforms,
                iteration_y,
                resolution,
                texture_over.clone(),
                texture_under.clone(),
            );
        }

        total_transforms.extend(transforms);

        for (transform, asset) in total_transforms {
            match asset {
                AssetType::Image(image_asset) => commands
                    .spawn_bundle(SpriteBundle {
                        transform: Transform::from_translation(transform),
                        sprite: sprite.clone(),
                        texture: image_asset,
                        ..Default::default()
                    })
                    .insert(MapComponent)
                    .insert(Background { processed: true }),
                AssetType::Svg(svg_asset) => commands
                    .spawn_bundle(Svg2dBundle {
                        transform: Transform::from_translation(transform),
                        svg: svg_asset,
                        origin: Origin::Center,
                        ..Default::default()
                    })
                    .insert(MapComponent)
                    .insert(Background { processed: false }),
            };
        }
    }
}

pub fn calculate_transforms_x(
    iter_x: u32,
    resolution: Vec2,
    texture: AssetType,
) -> Vec<(Vec3, AssetType)> {
    let positive_transform = Vec3::new((resolution.x - 1.0) * iter_x as f32, 0.0, 0.0);
    let negative_transform = positive_transform * -1.0;

    println!("{:?}", positive_transform);

    match positive_transform == negative_transform {
        true => vec![(positive_transform, texture)],
        false => vec![
            (positive_transform, texture.clone()),
            (negative_transform, texture),
        ],
    }
}

pub fn get_texture_for_index(texture_index: usize, texture_list: Vec<AssetType>) -> AssetType {
    let index = num::clamp(texture_index, 0, texture_list.len() - 1);
    texture_list[index].clone()
}

pub fn calculate_transforms_y(
    transforms: &mut Vec<(Vec3, AssetType)>,
    total_transforms: &mut Vec<(Vec3, AssetType)>,
    iter_y: u32,
    resolution: Vec2,
    texture_over: Vec<AssetType>,
    texture_under: Vec<AssetType>,
) {
    let texture_index = (iter_y - 1) as usize;
    let positive_texture = get_texture_for_index(texture_index, texture_over.clone());
    let negative_texture = get_texture_for_index(texture_index, texture_under.clone());

    for transform in transforms.clone() {
        let positive_y = resolution.y * iter_y as f32;
        let positive_transform = Vec3::new(0.0, positive_y, 0.) + transform.0;

        let negative_transform = positive_transform * Vec3::new(1., -1., 0.);

        total_transforms.push((positive_transform, positive_texture.clone()));
        total_transforms.push((negative_transform, negative_texture.clone()));
    }
}

pub fn modify_svg_background_transform(
    mut query_background: Query<(&mut Background, &mut Transform, &Handle<Svg>)>,
    resolution: Res<BackgroundResolution>,
    svg_assets: Res<Assets<Svg>>,
) {
    for (mut background, mut transform, svg_handle) in query_background.iter_mut() {
        if background.processed == false {
            let svg_asset = svg_assets.get(svg_handle);
            if svg_asset.is_some() {
                let asset = svg_asset.unwrap();

                let size_y = resolution.resolution.y / asset.size.y;
                let size_x = resolution.resolution.x / asset.size.x;

                transform.translation.z = 0.0;
                transform.scale = Vec3::new(size_x, size_y, 1.);
            }
        }
    }
}
