use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use anyhow;
use bevy::core::Stopwatch;
use bevy::math::Vec3Swizzles;
use bevy::prelude::*;
use bevy::render::render_resource::FilterMode;
use bevy::tasks::AsyncComputeTaskPool;
use bevy::utils::tracing::Instrument;
use bevy::window::WindowId;
use bevy::winit::WinitWindows;
use bevy_rapier2d::prelude::*;
use rand::prelude::SliceRandom;
use serde::{Deserialize, Serialize};
use winit::dpi::PhysicalSize;

use crate::background::create_bgs;
use crate::char::VictoryEvent;
use crate::collider::AACollider;
use crate::collider::*;
use crate::maps::{Map, SpriteAsset};
use crate::universal::*;
use crate::{
    AppStates, AssetDirectory, AugmentedFonts, CharComponentMap, CharacterInputMap, ConvertToRgb,
    Game, GameSettings, MapComponent, PlayerIdentifier, ProjectileIdentifier, ToPrimitive,
};

pub fn get_resolution(windows: Res<Windows>) -> Vec2 {
    let primary_window = windows.get_primary().unwrap();

    Vec2::new(
        primary_window.width() as f32,
        primary_window.height() as f32,
    )
}

pub struct GameRounds {
    pub previous_victory: VictoryEvent,
    pub total_victories: Vec<VictoryEvent>,
    pub previous_map_change: u64,
}

#[derive(Component)]
pub struct GameCountdownTextId {
    pub created_timestamp: f64,
    pub total_seconds: f64,
    pub fin_time: f64,
}

pub fn hide_update_game_text(
    mut countdown_text_query: Query<(&mut Visibility, &GameCountdownTextId)>,
    time: Res<Time>,
    settings: Res<GameSettings>,
) {
    for (mut visibility, id) in countdown_text_query.iter_mut() {
        let seconds_left = (settings.gameplay_settings.countdown_disappear
            - (time.seconds_since_startup() - id.fin_time))
            .ceil();

        if seconds_left <= 0.0 {
            visibility.is_visible = false;
        }
    }
}

#[derive(Component)]
pub struct VictoryText {
    pub created_timestamp: f64,
    pub total_seconds: f64,
}

pub fn change_map(
    mut game_resource: &mut ResMut<Game>,
    mut rounds: &mut ResMut<GameRounds>,
    settings: &Res<GameSettings>,
    map_query: &Query<(Entity), (With<MapComponent>)>,
    commands: &mut Commands,
) -> bool {
    let total_vic_len = rounds.total_victories.len() as u64;
    if (total_vic_len - rounds.previous_map_change)
        >= settings.gameplay_settings.rounds_to_map_change
    {
        rounds.previous_map_change = total_vic_len;
        println!(
            "length of available maps: {:#?}",
            game_resource.available_maps.len()
        );
        game_resource.selected_map = game_resource
            .available_maps
            .choose(&mut rand::thread_rng())
            .unwrap()
            .clone();
        println!("{:#?}", game_resource.selected_map.info.display_name);
        for (map_entity) in map_query.iter() {
            commands.entity(map_entity).despawn_recursive()
        }
        commands.remove_resource::<AssetDirectory>();
        true
    } else {
        false
    }
}

pub fn victory_screen(
    game: Res<Game>,
    game_rounds: Res<GameRounds>,
    font: Res<AugmentedFonts>,
    mut commands: Commands,
    time: Res<Time>,
    settings: Res<GameSettings>,
) {
    let text = match game_rounds.previous_victory {
        VictoryEvent::Victory(player_id) => {
            format!("P{} VICTORY", player_id)
        }
        VictoryEvent::Draw => String::from("DRAW"),
        VictoryEvent::None => String::from("NOBODY"),
    };

    commands
        .spawn_bundle(NodeBundle {
            style: Style {
                position_type: PositionType::Absolute,
                justify_content: JustifyContent::Center,
                margin: Rect::all(Val::Percent(5.0)),
                size: Size::new(Val::Percent(100.0), Val::Percent(100.0)),
                ..Default::default()
            },
            color: Color::NONE.into(),
            ..Default::default()
        })
        .with_children(|parent| {
            parent.spawn_bundle(TextBundle {
                style: Style {
                    align_self: AlignSelf::Center,
                    ..Default::default()
                },
                text: Text::with_section(
                    text,
                    TextStyle {
                        font: font.bold_font.clone(),
                        font_size: settings.window.height
                            / settings
                                .font_settings
                                .percentage_centre_text_size_to_window_height,
                        color: Color::from(
                            game.selected_map
                                .map_element_colours
                                .big_centre_text_colour
                                .convert_to_rgb(),
                        ),
                    },
                    Default::default(),
                ),

                ..Default::default()
            });
        })
        .insert(VictoryText {
            created_timestamp: time.seconds_since_startup(),
            total_seconds: settings.gameplay_settings.victory_disappear,
        });
}

pub fn spawn_game_victory(
    text_query: Query<(Entity), (With<CountDownTextNode>)>,
    map_query: Query<(Entity), (With<MapComponent>)>,
    player_query: Query<(Entity, &ColliderSyncEntity), (With<PlayerIdentifier>)>,
    victory_query: Query<
        (Entity, &VictoryText),
        (Without<CountDownTextNode>, Without<ColliderSyncEntity>),
    >,
    potential_projectile_query: Query<(Entity, &ColliderSyncEntity), (With<ProjectileIdentifier>)>,
    mut commands: Commands,
    mut game_resource: ResMut<Game>,
    mut rounds: ResMut<GameRounds>,
    time: Res<Time>,
    mut state: ResMut<State<AppStates>>,
    settings: Res<GameSettings>,
) {
    for (victory_entity, text) in victory_query.iter() {
        let seconds_left = (settings.gameplay_settings.victory_disappear
            - (time.seconds_since_startup() - text.created_timestamp))
            .ceil();

        if seconds_left > 0.0 {
            return;
        } else {
            commands.entity(victory_entity).despawn_recursive();
        }
    }

    //Next round

    if change_map(
        &mut game_resource,
        &mut rounds,
        &settings,
        &map_query,
        &mut commands,
    ) {
        // reload assets so we get the correct assets for our new map
        state.set(AppStates::LoadAssets);
    } else {
        state.set(AppStates::LoadChar);
    }

    for (entity, sync) in player_query.iter() {
        sync.despawn_self(&mut commands);
        commands.entity(entity).despawn_recursive();
    }

    for entity in text_query.iter() {
        commands.entity(entity).despawn_recursive();
    }

    for (entity, sync) in potential_projectile_query.iter() {
        sync.despawn_self(&mut commands);
        commands.entity(entity).despawn_recursive();
    }

    // Despawn all unwanted resources
    commands.remove_resource::<CharComponentMap>();
    commands.remove_resource::<CharacterInputMap>();
    // Insert new versions of the resources
    commands.insert_resource(CharComponentMap::new());
    commands.insert_resource(CharacterInputMap::new());
}

pub fn update_game_countdown_to_start(
    mut countdown_text_query: Query<(&mut Text, &mut GameCountdownTextId)>,
    time: Res<Time>,
    settings: Res<GameSettings>,
    mut state: ResMut<State<AppStates>>,
) {
    for (mut countdown_text, mut id) in countdown_text_query.iter_mut() {
        let seconds_left =
            (id.total_seconds - (time.seconds_since_startup() - id.created_timestamp)).ceil();
        let mut seconds_text = format!("{}", seconds_left.to_u64().unwrap_or(0));

        if seconds_left <= 0.0 {
            seconds_text = String::from("GO")
        }

        for mut countdown_text in &mut countdown_text.sections {
            countdown_text.value = seconds_text.clone()
        }

        if seconds_left <= 0.0 {
            id.fin_time = time.seconds_since_startup();
            state.set(AppStates::LoadGame);
        }
    }
}

#[derive(Component)]
pub struct CountDownTextNode;

pub fn spawn_game_countdown_to_start(
    game: Res<Game>,
    font: Res<AugmentedFonts>,
    mut commands: Commands,
    time: Res<Time>,
    settings: Res<GameSettings>,
) {
    commands
        .spawn_bundle(NodeBundle {
            style: Style {
                position_type: PositionType::Absolute,
                justify_content: JustifyContent::Center,
                margin: Rect::all(Val::Percent(5.0)),
                size: Size::new(Val::Percent(100.0), Val::Percent(100.0)),
                ..Default::default()
            },
            color: Color::NONE.into(),
            ..Default::default()
        })
        .with_children(|parent| {
            parent
                .spawn_bundle(TextBundle {
                    style: Style {
                        align_self: AlignSelf::Center,
                        ..Default::default()
                    },
                    text: Text::with_section(
                        format!(
                            "{}",
                            settings
                                .gameplay_settings
                                .countdown_to_start_time
                                .floor()
                                .to_u64()
                                .expect("Failed to convert countdown time to u64 :skull:")
                        ),
                        TextStyle {
                            font: font.bold_font.clone(),
                            font_size: settings.window.height
                                / settings
                                    .font_settings
                                    .percentage_centre_text_size_to_window_height,
                            color: Color::from(
                                game.selected_map
                                    .map_element_colours
                                    .big_centre_text_colour
                                    .convert_to_rgb(),
                            ),
                        },
                        Default::default(),
                    ),

                    ..Default::default()
                })
                .insert(GameCountdownTextId {
                    created_timestamp: time.seconds_since_startup(),
                    total_seconds: settings.gameplay_settings.countdown_to_start_time,
                    fin_time: 0.0,
                });
        })
        .insert(CountDownTextNode);
}

pub fn get_info_scale_resolution(window_descriptor: &Res<WindowDescriptor>, info: &Info) -> Vec2 {
    let width_ratio = window_descriptor.width as f32 / info.base_dimensions[0];
    let height_ratio = window_descriptor.height as f32 / info.base_dimensions[1];

    Vec2::new(width_ratio, height_ratio)
}

pub fn get_screen_dims(windows_winit: &WinitWindows, window_id: WindowId) -> PhysicalSize<u32> {
    let primary_window = windows_winit.get_window(window_id).unwrap();
    let size = primary_window.current_monitor().unwrap().size();

    size
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct WinitWindowsInfo {
    pub screen_dim: Vec2,
}
