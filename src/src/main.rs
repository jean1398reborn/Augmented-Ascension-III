#![windows_subsystem = "windows"]

use std::collections::HashMap;
use std::fmt::format;
use std::future::Future;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4};
use std::ops::{Add, Index};
use std::sync::Arc;

use async_compat::Compat;
use bevy::asset::AssetServerSettings;
use bevy::diagnostic::{FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin};
use bevy::ecs::query::QueryEntityError;
use bevy::ecs::system::QuerySingleError;
use bevy::input::keyboard::KeyboardInput;
use bevy::math::{Mat2, Vec3Swizzles};
use bevy::prelude::*;
use bevy::reflect::List;
use bevy::render::camera::ScalingMode;
use bevy::sprite::{MaterialMesh2dBundle, Mesh2dHandle};
use bevy::tasks::AsyncComputeTaskPool;
use bevy::window::{WindowId, WindowMode, WindowResizeConstraints};
use bevy::winit::WinitWindows;
use bevy_inspector_egui::widgets::ResourceInspector;
use bevy_inspector_egui::{Inspectable, RegisterInspectable, WorldInspectorPlugin};
use bevy_inspector_egui_rapier::InspectableRapierPlugin;
use bevy_rapier2d::prelude::*;
use bevy_rapier2d::rapier::prelude::CollisionEventFlags;
use bevy_svg::prelude::*;
use bitflags::bitflags;
use igd::aio;
use igd::aio::Gateway;
use igd::{AddPortError, GetExternalIpError, PortMappingProtocol, SearchError, SearchOptions};
use nalgebra::{Isometry2, Vector2};
use num;
use num::{Float, ToPrimitive};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::process::Command;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net;
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::sync::RwLock;
use toml::ser::Error;
use winit::dpi::PhysicalSize;

use crate::assets::{
    add_augmented_fonts, retrieve_asset_maps, update_image_sampler, update_svg_transforms, Asset,
    AssetDirectory, AssetInfoMap, AugmentedFonts, InterpolateHandles, SvgInfo,
};
use crate::background::modify_svg_background_transform;
use crate::char::{
    check_cooldowns, check_victory_conditions, healthbar_update, load_character,
    load_selected_characters, sync_objects_colliders, update_dir_look, AAPlayerDescriptor,
    AttackIdentifierTextId, AttackInstanceDirectory, CharComponentMap,
    CharComponentPlayerIdentifier, CharEntities, Character, ConvertToRgb, CooldownMap,
    DirectionFacingFlags, MoveDirLookIdentifier, PlayerHealth, PlayerHealthBarId, PlayerIdentifier,
    SyncTransformOffset, VictoryEvent,
};
use crate::draw::*;
use crate::game::*;
use crate::maps::Map;
use crate::maps::*;
use crate::universal::*;
use crate::AppStates::{LoadMap, PreGame};
use crate::CoreStage::{First, Last, Update};
use crate::KeyCode::Back;

mod action_traits;
mod assets;
mod background;
mod char;
mod collider;
mod draw;
mod game;
mod maps;
mod post_processing;
mod projectile;
mod rigidbody;
mod universal;

use crate::collider::{ColliderSyncEntity, SyncColliderFlags};
use crate::projectile::{
    attack_text_update, execute_unused_actions, projectile_lifetimes, ProjectileIdentifier,
};
use bevy_mod_rounded_box::*;
use futures_lite::StreamExt;
use game::*;
use image::{GenericImageView, ImageResult};
use rand::prelude::SliceRandom;
use winit::window::Icon;

pub const RAPIERSCALE: f32 = 40.0;

pub struct RapierScaleConfig {
    scale: f32,
}

fn main() {

    // create main app

    let asset_server_settings = AssetServerSettings {
        asset_folder: "".into(),
        watch_for_changes: false,
    };

    let mut interpolate_handles = InterpolateHandles { handles: vec![] };

    let mut app = App::new();

    let mut game_settings: GameSettings = match toml::from_str(&recieve_settings()) {
        Ok(settings) => settings,
        Err(_) => {
            match std::fs::rename(SETTINGSPATH, format!("{}_old", SETTINGSPATH)) {
                Ok(_) => {}
                Err(_) => {}
            };
            toml::from_str(&recieve_settings()).unwrap()
        }
    };

    app.insert_resource(AttackInstanceDirectory {
        attack_instances: HashMap::new(),
        unexecuted_actions: vec![],
        previous_used_id: 0,
        cooldown: CooldownMap::new(),
    });
    app.insert_resource(AAGamePlayToggle {
        process_movement: true,
    });

    app.insert_resource(WindowDescriptor {
        width: game_settings.window.width,
        height: game_settings.window.height,
        title: "Augmented Ascension".to_string(),
        ..Default::default()
    });
    app.insert_resource(AssetInfoMap::new());
    app.insert_resource(RapierScaleConfig { scale: RAPIERSCALE });
    app.insert_resource(asset_server_settings);
    app.insert_resource(GameRounds {
        previous_victory: VictoryEvent::None,
        total_victories: vec![],
        previous_map_change: 0,
    });
    // run the installer
    let child = Command::new("assets/installer.exe")
        .spawn();
    app.insert_resource(Msaa { samples: 4 });
    app.insert_resource(interpolate_handles);
    app.insert_resource(CharComponentMap::new());
    app.insert_resource(CharacterInputMap::new());
    app.add_startup_system(add_augmented_fonts);
    app.add_plugins(DefaultPlugins);
    app.add_plugin(SvgPlugin);
    app.add_plugin(InspectableRapierPlugin);

    // Physics plugin
    let mut physics_plugin = RapierPhysicsPlugin::<NoUserData>::default();
    app.add_plugin(physics_plugin.with_physics_scale(RAPIERSCALE));

    if game_settings.special_settings.debug_mode {
        app.add_plugin(RapierDebugRenderPlugin::default());
        app.add_plugin(WorldInspectorPlugin::new());
    }

    // bevy_inspector_egui things
    app.register_inspectable::<UpdatedTransformComponent>();
    app.register_inspectable::<PlayerIdentifier>();
    app.register_inspectable::<VelocityForceCap>();
    app.register_inspectable::<SyncTransformOffset>();
    app.register_inspectable::<SyncColliderFlags>();
    app.register_inspectable::<PlayerHealth>();
    app.register_inspectable::<ProjectileIdentifier>();

    // to make a vignette 💀
    if game_settings.special_settings.vignette {
        post_processing::post_processing(&mut app);
    }

    //app.add_startup_system(server_test);
    app.add_startup_system(window_icon);
    app.add_startup_system(create_camera);
    app.add_startup_system(get_winit_information.exclusive_system());
    app.add_system_to_stage(Update, update_svg_transforms);
    app.add_system_to_stage(Update, sync_objects_colliders);
    app.add_system_to_stage(Last, cap_velocity);
    app.add_system_to_stage(PhysicsStages::Writeback, cap_velocity);
    let initialisation_system_set = SystemSet::on_enter(AppStates::LoadComps)
        .with_system(maps::load_maps)
        .with_system(char::load_characters);
    app.add_state(AppStates::LoadComps);
    app.add_system_set(initialisation_system_set);
    //app.add_system(my_cursor_system);

    //Main menu and ui stuff should be inserted and started after loadcomps.

    let selectmap_set = SystemSet::on_enter(AppStates::PreLoad).with_system(select_map);
    app.add_system_set(selectmap_set);

    let load_asset_set =
        SystemSet::on_enter(AppStates::LoadAssets).with_system(retrieve_asset_maps);
    app.add_system_set(load_asset_set);

    let load_game_set = SystemSet::on_enter(AppStates::LoadMap).with_system(load_map);
    app.add_system_set(load_game_set);

    app.add_system_set(
        SystemSet::on_update(AppStates::MainMenu)
            .with_system(input_selector)
            .with_system(update_selector_bar_mm),
    );

    app.add_system_set(SystemSet::on_enter(AppStates::MainMenu).with_system(spawn_main_menu));

    app.add_system_set(
        SystemSet::on_enter(AppStates::LoadChar).with_system(load_selected_characters),
    );

    app.add_system_set(
        SystemSet::on_enter(AppStates::PreGame)
            .with_system(pause_physics_and_movement)
            .with_system(spawn_game_countdown_to_start),
    );

    app.add_system_set(SystemSet::on_enter(AppStates::Quit).with_system(quit));

    app.add_system_set(
        SystemSet::on_enter(AppStates::PlayersIdentify).with_system(spawn_player_identify),
    );

    app.add_system_set(
        SystemSet::on_enter(AppStates::SelectCharacter).with_system(select_character_spawn_menu),
    );

    app.add_system_set(
        SystemSet::on_update(AppStates::SelectCharacter).with_system(selected_char_selector)
    );

    app.add_system_set(
        SystemSet::on_update(AppStates::PlayersIdentify).with_system(player_identifier_adder)
    );

    app.add_system_set(
        SystemSet::on_update(AppStates::PreGame).with_system(update_game_countdown_to_start),
    );

    app.add_system_set(
        SystemSet::on_enter(AppStates::LoadGame).with_system(resume_physics_and_movement),
    );

    let on_game_set = SystemSet::on_update(AppStates::LoadGame)
        .with_system(update_image_sampler)
        .with_system(check_cooldowns)
        .with_system(modify_svg_background_transform)
        .with_system(update_dir_look)
        .with_system(check_victory_conditions)
        .with_system(enforce_char_collision_dominance)
        .with_system(healthbar_update)
        .with_system(hide_update_game_text)
        .with_system(attack_text_update)
        .with_system(execute_unused_actions)
        .with_system(projectile_lifetimes)
        .with_system(health_despawn_check)
        .with_system(collision_process);

    app.add_system_set(
        SystemSet::on_enter(AppStates::Victory)
            .with_system(victory_screen)
            .with_system(pause_physics_and_movement),
    );
    app.add_system_set(SystemSet::on_update(AppStates::Victory).with_system(spawn_game_victory));

    app.add_system_set_to_stage(Update, on_game_set);
    app.add_system_to_stage(First, movement_input_system);
    //run ze app
    app.insert_resource(game_settings);
    app.run();
}

pub fn pause_physics_and_movement(
    mut physics_config: ResMut<RapierConfiguration>,
    mut gameplay_toggles: ResMut<AAGamePlayToggle>,
) {
    physics_config.physics_pipeline_active = false;
    gameplay_toggles.process_movement = false;
}

pub fn resume_physics_and_movement(
    mut physics_config: ResMut<RapierConfiguration>,
    mut gameplay_toggles: ResMut<AAGamePlayToggle>,
    mut keycodes: ResMut<Input<KeyCode>>,
) {
    *keycodes = Input::<KeyCode>::default();
    physics_config.physics_pipeline_active = true;
    gameplay_toggles.process_movement = true;
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub enum AppStates {
    // Clean UI Main Menu, CumMenu (CamelCase)
    LoadComps,
    MainMenu,
    PreLoad,
    PreGame,
    LoadMap,
    LoadChar,
    LoadGame,
    LoadAssets,
    Victory,
    Quit,
    Settings,
    PlayersIdentify,
    SelectCharacter,
}

pub fn quit() {
    std::process::exit(1);
}

#[derive(Component)]
pub struct UiCamera;

fn create_camera(mut commands: Commands, asset: Res<AssetServer>) {
    let mut camera_2d = OrthographicCameraBundle::new_2d();
    let mut camera_ui = UiCameraBundle::default();
    commands.spawn_bundle(camera_2d).insert(MainCamera);
    commands.spawn_bundle(camera_ui).insert(UiCamera);
}

#[derive(Component)]
pub struct MainCamera;

pub struct SelectedCharacters {
    pub characters: HashMap<u64, Character>,
}

pub type MenuInputScheme = HashMap<KeyCode, Vec<InputPurpose>>;

pub struct MainMenuSelected {
    pub total_options: Vec<String>,
    pub current_selected_id: usize,
    pub input_scheme: MenuInputScheme,
    pub switch_state: Vec<AppStates>,
}

#[derive(Component)]
pub struct SelectorBar;

#[derive(Component)]
pub struct MainMenuComponent;

pub fn spawn_main_menu(
    mut commands: Commands,
    asset: Res<AssetServer>,
    settings: Res<GameSettings>,
    fonts: Res<AugmentedFonts>,
    window: Res<WinitWindowsInfo>,
) {
    // code harder than diamond

    let main_big_asset: Handle<Image> = asset.load(&settings.main_menu.main_sprite);
    commands
        .spawn_bundle(SpriteBundle {
            sprite: Sprite {
                custom_size: Some(
                    Vec2::new(settings.window.height, settings.window.height)
                        * Vec2::from(settings.main_menu.main_sprite_size),
                ),
                ..Default::default()
            },
            transform: Transform::from_translation(Vec3::new(
                0.,
                settings.window.height * settings.main_menu.main_sprite_window_vertical,
                50.,
            )),
            texture: main_big_asset,
            ..Default::default()
        })
        .insert(MainMenuComponent);

    let select_options = vec![
        "PLAY".to_string(),
        "QUIT".to_string(),
    ];

    let select_option_style = TextStyle {
        font: fonts.bold_font.clone(),
        font_size: settings.window.height * settings.main_menu.select_option_font_size,
        color: Color::from(
            settings
                .main_menu
                .select_option_font_colour
                .convert_to_rgb(),
        ),
    };

    let text_alignment = TextAlignment {
        vertical: VerticalAlign::Center,
        horizontal: HorizontalAlign::Center,
    };

    let mut current_selection_padding_down = 0.0;

    for select_option in &select_options {
        commands
            .spawn_bundle(Text2dBundle {
                text: Text::with_section(
                    select_option,
                    select_option_style.clone(),
                    text_alignment.clone(),
                ),
                transform: Transform::from_translation(Vec3::new(
                    0.,
                    (settings.window.height * settings.main_menu.select_option_window_down)
                        + (current_selection_padding_down
                            * settings.main_menu.select_option_individual_distance
                            * settings.window.height),
                    50.,
                )),
                ..Default::default()
            })
            .insert(MainMenuComponent);

        current_selection_padding_down += 1.0;
    }

    commands
        .spawn_bundle(SpriteBundle {
            sprite: Sprite {
                color: Color::from(settings.main_menu.highlight_colour.convert_to_rgb()),
                custom_size: Some(Vec2::new(
                    window.screen_dim.x,
                    settings.main_menu.highlight_height * settings.window.height,
                )),
                ..Default::default()
            },
            transform: Transform::from_translation(Vec3::new(
                0.,
                (settings.window.height * settings.main_menu.select_option_window_down),
                20.,
            )),
            ..Default::default()
        })
        .insert(SelectorBar)
        .insert(MainMenuComponent);

    commands.insert_resource(MainMenuSelected {
        total_options: select_options,
        current_selected_id: 0,
        input_scheme: reverse_char_input_purpose(&settings),
        switch_state: vec![
            AppStates::PlayersIdentify,
            AppStates::Quit,
        ],
    });
}

pub struct PlayerIdentify {
    pub taken: bool,
    pub movement_player_id: u64,
    pub assigned_player_id: u64,
    pub input_settings: CharacterInputSettings,
}

pub type PlayerIdentifierScheme = HashMap<KeyCode, PlayerIdentify>;

pub fn identifier_char_input_purpose(game_settings: &Res<GameSettings>) -> PlayerIdentifierScheme {
    let mut input_purpose_map = PlayerIdentifierScheme::new();
    let ctrls = [
        (&game_settings.p1_ctrls, 1),
        (&game_settings.p2_ctrls, 2),
        (&game_settings.p3_ctrls, 3),
        (&game_settings.p4_ctrls, 4),
        (&game_settings.p5_ctrls, 5),
        (&game_settings.p6_ctrls, 6),
        (&game_settings.p7_ctrls, 7),
        (&game_settings.p8_ctrls, 8),
    ];

    for (ctrl, id) in ctrls {
        for (keycode, input_purpose) in ctrl.into_iter() {
            match input_purpose {
                InputPurpose::Up => {
                    input_purpose_map
                        .entry(keycode.clone())
                        .or_insert(PlayerIdentify {
                            taken: false,
                            movement_player_id: id as u64,
                            assigned_player_id: 0,
                            input_settings: *ctrl,
                        });
                }
                _ => {}
            }
        }
    }

    input_purpose_map
}

pub fn reverse_char_input_purpose(game_settings: &Res<GameSettings>) -> MenuInputScheme {
    let mut input_purpose_map = MenuInputScheme::new();
    let ctrls = [
        &game_settings.p1_ctrls,
        &game_settings.p2_ctrls,
        &game_settings.p3_ctrls,
        &game_settings.p4_ctrls,
        &game_settings.p5_ctrls,
        &game_settings.p6_ctrls,
        &game_settings.p7_ctrls,
        &game_settings.p8_ctrls,
    ];

    for ctrl in ctrls {
        for (keycode, input_purpose) in ctrl.into_iter() {
            let input_vec = input_purpose_map.entry(keycode).or_insert(vec![]);
            input_vec.push(input_purpose.clone());
        }
    }

    input_purpose_map
}

pub fn update_selector_bar_mm(
    selected: Res<MainMenuSelected>,
    mut selector_bar: Query<&mut Transform, With<SelectorBar>>,
    settings: Res<GameSettings>,
) {
    for mut selector_bar_transform in selector_bar.iter_mut() {
        selector_bar_transform.translation.y = (settings.main_menu.select_option_window_down
            * settings.window.height)
            + (selected.current_selected_id as f32
                * settings.main_menu.select_option_individual_distance
                * settings.window.height);
    }
}

pub fn input_selector(
    mut input: ResMut<Input<KeyCode>>,
    mut selected: ResMut<MainMenuSelected>,
    mut state: ResMut<State<AppStates>>,
    main_menu_components: Query<Entity, With<MainMenuComponent>>,
    mut commands: Commands,
) {
    let mut selected_id = selected.current_selected_id;

    for (keycode, actions) in selected.input_scheme.iter() {
        if input.just_pressed(*keycode) {
            for action in actions {
                match action {
                    InputPurpose::Up => {
                        if selected_id > 0 {
                            selected_id -= 1;
                        } else {
                            selected_id = selected.total_options.len() - 1;
                        }
                    }
                    InputPurpose::Down => {
                        if selected_id < selected.total_options.len() - 1 {
                            selected_id += 1;
                        } else {
                            selected_id = 0;
                        }
                    }
                    InputPurpose::Atk1 => {
                        for entity in main_menu_components.iter() {
                            commands.entity(entity).despawn_recursive();
                        }
                        state.set(selected.switch_state[selected_id as usize].clone());
                    }
                    _ => {}
                }
            }
        }
    }

    selected.current_selected_id = selected_id;
}

pub struct PlayerIdentifierMenu {
    pub player_count: usize,
}

#[derive(Component)]
pub struct PlayerIdMenuComponent;

pub fn spawn_player_identify(
    mut commands: Commands,
    settings: Res<GameSettings>,
    fonts: Res<AugmentedFonts>,
    window: Res<WinitWindowsInfo>,
) {
    let text_alignment = TextAlignment {
        vertical: VerticalAlign::Center,
        horizontal: HorizontalAlign::Center,
    };

    let player_tooltip_style = TextStyle {
        font: fonts.bold_font.clone(),
        font_size: settings.window.height * settings.player_id.help_text_size,
        color: Color::from(settings.player_id.help_text_colour.convert_to_rgb()),
    };

    let bottom_tooltip_style = TextStyle {
        font: fonts.bold_font.clone(),
        font_size: settings.window.height * settings.player_id.bottom_tooltip_size,
        color: Color::from(settings.player_id.bottom_tooltip_colour.convert_to_rgb()),
    };

    commands.spawn_bundle(Text2dBundle {
        text: Text::with_section(
            format!("Press UP to join and Press {:?} to start", settings.player_id.start_key),
            player_tooltip_style.clone(),
            text_alignment.clone(),
        ),
        transform: Transform::from_translation(Vec3::new(
            0.,
            settings.window.height * settings.player_id.help_text_vertical,
            30.,
        )),
        ..Default::default()
    }).insert(PlayerIdMenuComponent);

    commands.spawn_bundle(Text2dBundle {
        text: Text::with_section(
            "A minimum of two players are required",
            player_tooltip_style.clone(),
            text_alignment.clone(),
        ),
        transform: Transform::from_translation(Vec3::new(
            0.,
            settings.window.height * settings.player_id.bottom_tooltip_height,
            30.,
        )),
        ..Default::default()
    }).insert(PlayerIdMenuComponent);

    commands.spawn_bundle(SpriteBundle {
        sprite: Sprite {
            color: Color::from(settings.player_id.highlight_colour.convert_to_rgb()),
            custom_size: Some(Vec2::new(
                window.screen_dim.x,
                settings.player_id.highlight_height * settings.window.height,
            )),
            ..Default::default()
        },
        transform: Transform::from_translation(Vec3::new(
            0.,
            (settings.window.height * settings.player_id.help_text_vertical),
            20.,
        )),
        ..Default::default()
    }).insert(PlayerIdMenuComponent);

    commands.insert_resource(PlayerIdentifierMenu { player_count: 0 });

    commands.insert_resource(identifier_char_input_purpose(&settings));
    commands.insert_resource(CharacterInputIdentifierMap {
        map: HashMap::new(),
    });
}

pub struct CharacterInputIdentifierMap {
    pub map: HashMap<u64, CharacterInputSettings>,
}

pub fn player_identifier_adder(
    input: Res<Input<KeyCode>>,
    mut player_identify_menu: ResMut<PlayerIdentifierMenu>,
    mut character_input_map: ResMut<CharacterInputIdentifierMap>,
    mut player_identify_map: ResMut<PlayerIdentifierScheme>,
    player_identify: Query<Entity, With<PlayerIdMenuComponent>>,
    mut state: ResMut<State<AppStates>>,
    settings: Res<GameSettings>,
    fonts: Res<AugmentedFonts>,
    window: Res<WinitWindowsInfo>,
    mut commands: Commands,
) {
    for (keycode, mut player_identify) in player_identify_map.iter_mut() {
        if input.just_pressed(*keycode) {
            if !player_identify.taken {
                player_identify.taken = true;
                let text_alignment = TextAlignment {
                    vertical: VerticalAlign::Center,
                    horizontal: HorizontalAlign::Center,
                };

                let player_style = TextStyle {
                    font: fonts.bold_font.clone(),
                    font_size: settings.window.height * settings.player_id.player_joined_font_size,
                    color: Color::from(
                        settings
                            .player_id
                            .player_joined_font_colour
                            .convert_to_rgb(),
                    ),
                };

                commands.spawn_bundle(Text2dBundle {
                    text: Text::with_section(
                        format!("P{}", player_identify_menu.player_count + 1),
                        player_style.clone(),
                        text_alignment.clone(),
                    ),
                    transform: Transform::from_translation(Vec3::new(
                        0.,
                        (settings.window.height * settings.player_id.player_joined_window_down) + (
                            player_identify_menu.player_count as f32 * settings.window.height * settings.player_id.player_joined_individual_distance
                        ),
                        30.,
                    ) ),
                    ..Default::default()
                }).insert(PlayerIdMenuComponent);
                player_identify_menu.player_count += 1;
                player_identify.assigned_player_id = player_identify_menu.player_count as u64;
                character_input_map.map.insert(
                    player_identify.assigned_player_id,
                    player_identify.input_settings,
                );
            }
        }
    }

    if input.just_pressed(settings.player_id.start_key) & (player_identify_menu.player_count > 1) {
        for entity in player_identify.iter() {
            commands.entity(entity).despawn_recursive();
        }
        commands.insert_resource(SelectedCharacters {
            characters: HashMap::new()
        });
        commands.insert_resource(TotalCharactersSelect {
            total_to_select: player_identify_menu.player_count as u64,
        next_player:1});
        state.set(AppStates::SelectCharacter);
    };
}

pub struct TotalCharactersSelect {
    pub total_to_select: u64,
    pub next_player: u64,
}

pub struct SelectCharacterSpawnMenu {
    pub currently_selected_character: usize,
    pub current_player_id: u64,
    pub input_scheme: MenuInputScheme,
}

pub struct CharacterIcons(HashMap<usize, Handle<Image>>);

pub fn get_selected_character_icons(
    asset: &Res<AssetServer>,
    chars: &Res<Vec<Character>>,
    commands: &mut Commands
) -> HashMap<usize, Handle<Image>> {

    let mut character_icons = HashMap::new();

    for (index, char) in chars.iter().enumerate() {
        let image : Handle<Image> = asset.load(&char.info.icon);
        character_icons.insert(index, image);
    }

    commands.insert_resource(
        CharacterIcons(character_icons.clone())
    );

    character_icons


}

#[derive(Component)]
pub struct SelectCharacterMenuComponent;

pub fn select_character_spawn_menu(
    chars: Res<Vec<Character>>,
    mut commands: Commands,
    mut state: ResMut<State<AppStates>>,
    settings: Res<GameSettings>,
    fonts: Res<AugmentedFonts>,
    window: Res<WinitWindowsInfo>,
    total_to_select: Res<TotalCharactersSelect>,
    asset: Res<AssetServer>,
) {

    let mut character_icons = get_selected_character_icons(
        &asset,
            &chars,
        &mut commands
    );

    let mut select_character_menu = SelectCharacterSpawnMenu {
        currently_selected_character: 0,
        current_player_id: total_to_select.next_player,
        input_scheme: reverse_char_input_purpose(
            &settings,
        ),
    };

    spawn_current_selected_character(
        &window,
        select_character_menu.current_player_id,
        select_character_menu.currently_selected_character,
        &chars,
        &mut commands,
        &settings,
        &fonts,
        &mut character_icons,
        &asset,
    );

    commands.insert_resource(
        select_character_menu
    );
}

#[derive(Component)]
pub struct CurrentCharacterSelectedId;

pub fn spawn_current_selected_character(
    window: &Res<WinitWindowsInfo>,
    current_player_id: u64,
    selected_character: usize,
    chars: &Res<Vec<Character>>,
    commands: &mut Commands,
    settings: &Res<GameSettings>,
    fonts: &Res<AugmentedFonts>,
    selected_icons: &mut HashMap<usize, Handle<Image>>,
    asset: &Res<AssetServer>,
) {
    let current_selected_style = TextStyle {
        font: fonts.bold_font.clone(),
        font_size: settings.window.height * settings.select_char.current_selected_char_text_size,
        color: Color::from(settings.select_char.current_selected_char_colour.convert_to_rgb()),
    };

    let text_alignment = TextAlignment {
        vertical: VerticalAlign::Center,
        horizontal: HorizontalAlign::Center,
    };

    let current_player_style = TextStyle {
        font: fonts.bold_font.clone(),
        font_size: settings.window.height * settings.select_char.current_player_text_size,
        color: Color::from(settings.select_char.current_player_text_colour.convert_to_rgb()),
    };

    commands.spawn_bundle(Text2dBundle {
        text: Text::with_section(
            format!("P{} Choose Your Character", current_player_id),
            current_player_style.clone(),
            text_alignment.clone(),
        ),
        transform: Transform::from_translation(Vec3::new(
            0.,
            settings.window.height * settings.select_char.current_player_text_up_dist,
            30.,
        )),
        ..Default::default()
    }).insert(SelectCharacterMenuComponent).insert(CurrentCharacterSelectedId);

    commands.spawn_bundle(SpriteBundle {
        sprite: Sprite {
            color: Color::from(settings.select_char.current_player_text_highlight_colour.convert_to_rgb()),
            custom_size: Some(Vec2::new(
                window.screen_dim.x,
                settings.select_char.current_player_text_highlight_height * settings.window.height,
            )),
            ..Default::default()
        },
        transform: Transform::from_translation(Vec3::new(
            0.,
            (settings.window.height * settings.select_char.current_player_text_up_dist),
            20.,
        )),
        ..Default::default()
    }).insert(SelectCharacterMenuComponent).insert(CurrentCharacterSelectedId);

    commands.spawn_bundle(SpriteBundle {
        sprite: Sprite {
            custom_size: Some(Vec2::new(settings.window.height, settings.window.height) * Vec2::from(settings.select_char.character_icon_size)),
            ..Default::default()
        },
        transform: Transform::from_translation(Vec3::new(0., settings.window.height * settings.select_char.icon_vertical_dist, 30.)),
        texture: selected_icons.entry(selected_character).or_insert(
            asset.load(&chars[selected_character].info.icon)
        ).clone(),
        ..Default::default()
    }).insert(CurrentCharacterSelectedId);



    commands.spawn_bundle(Text2dBundle {
        text: Text::with_section(
            format!("{}", chars[selected_character].info.display_name),
            current_selected_style.clone(),
            text_alignment.clone(),
        ),
        transform: Transform::from_translation(Vec3::new(0., settings.window.height * settings.select_char.current_selected_char_up_dist, 30.)),
        ..Default::default()
    }).insert(CurrentCharacterSelectedId);
}

pub fn selected_char_selector(
    mut input: ResMut<Input<KeyCode>>,
    mut selected: ResMut<SelectedCharacters>,
    mut selected_character_menu: ResMut<SelectCharacterSpawnMenu>,
    mut state: ResMut<State<AppStates>>,
    chars: Res<Vec<Character>>,
    window: Res<WinitWindowsInfo>,
    mut char_icons: ResMut<CharacterIcons>,
    settings: Res<GameSettings>,
    fonts: Res<AugmentedFonts>,
    asset: Res<AssetServer>,
    current_char_components: Query<Entity, With<CurrentCharacterSelectedId>>,
    mut commands: Commands,
    mut total_characters: ResMut<TotalCharactersSelect>
) {
    let mut selected_id = selected_character_menu.currently_selected_character;
    let mut reset_state = false;
    let mut changed_select = false;

    for (keycode, actions) in selected_character_menu.input_scheme.iter() {
        if input.just_pressed(*keycode) {
            for action in actions {
                match action {
                    InputPurpose::Up => {
                        if selected_id > 0 {
                            selected_id -= 1;
                            changed_select = true;
                        } else {
                            selected_id = chars.len() - 1;
                            changed_select = true;
                        }
                    }
                    InputPurpose::Down => {
                        if selected_id < chars.len() - 1 {
                            selected_id += 1;
                            changed_select = true;
                        } else {
                            selected_id = 0;
                            changed_select = true;
                        }
                    }
                    InputPurpose::Atk1 => {
                        *input = Input::<KeyCode>::default();
                        selected.characters.insert(
                            selected_character_menu.current_player_id,
                            chars[selected_id].clone()
                        );

                        reset_state = true;
                        changed_select = true;
                        total_characters.next_player += 1;
                    }
                    _ => {}
                }
            }
        }
    }
    selected_character_menu.currently_selected_character = selected_id;

    if changed_select {
        for current_char_entity in current_char_components.iter() {
            commands.entity(current_char_entity).despawn_recursive()
        }
        if total_characters.next_player > total_characters.total_to_select {
            state.set(AppStates::PreLoad);
            return
        }
        if reset_state {
            commands.remove_resource::<SelectCharacterSpawnMenu>();
            state.restart();
            return
        }
        spawn_current_selected_character(
            &window,
            selected_character_menu.current_player_id,
            selected_character_menu.currently_selected_character,
            &chars,
            &mut commands,
            &settings,
            &fonts,
            &mut char_icons.0,
            &asset,
        );

    }
}


pub fn select_map(
    maps: Res<Vec<Map>>,
    chars: Res<SelectedCharacters>,
    mut commands: Commands,
    mut state: ResMut<State<AppStates>>,
) {
    let available_maps = maps.into_inner().clone();
    let selected_map = available_maps
        .choose(&mut rand::thread_rng())
        .unwrap()
        .clone();

    let mut game = Game {
        selected_characters: chars.characters.clone(),
        available_characters: vec![],
        selected_map,
        available_maps,
    };

    commands.insert_resource(game);
    state.set(AppStates::LoadAssets);
}


pub fn get_winit_information(world: &mut World) {
    // Grab winit windows resource for getting descriptive information on the window properties such as the id for the window,,
    // The size of the monitor etc.
    let winit_windows = match world.get_non_send_resource::<WinitWindows>() {
        Some(winit_windows) => winit_windows,
        None => panic!("Failed to get Winit Windows: WinitWindows Resource does not exist."),
    };

    let windows = match world.get_resource::<Windows>() {
        Some(windows) => windows,
        None => panic!("Failed to get Windows: Windows resource does not exist."),
    };

    let primary_window = match windows.get_primary() {
        Some(primary_window) => primary_window,
        None => panic!("Failed to find the primary window used."),
    };

    let primary_id = primary_window.id();
    let resolution = get_screen_dims(winit_windows, primary_id); // hello??? my laptop is so fuckign bad autocorrect and auto typing from compiler inferrence takes a millenium >:(

    world.insert_resource(WinitWindowsInfo {
        screen_dim: Vec2::new(resolution.width as f32, resolution.height as f32),
    })
}

pub fn window_icon(winit_windows: NonSend<WinitWindows>, game_settings: Res<GameSettings>) {
    let primary_window = match winit_windows.get_window(WindowId::primary()) {
        None => {
            warn!("Unable to get primary window in set window icon function");
            return;
        }
        Some(primary_window) => primary_window,
    };

    let (icon_rgba, icon_width, icon_height) = {
        let image = match image::open(game_settings.window.icon_path.clone()) {
            Ok(image) => image,
            Err(error) => {
                warn!("Failed to load icon image {}", error);
                return;
            }
        };

        let (width, height) = image.dimensions();
        let rgba = image.into_rgba8().into_raw();
        (rgba, width, height)
    };

    let icon = Icon::from_rgba(icon_rgba, icon_width, icon_height).unwrap();
    primary_window.set_window_icon(Some(icon));
}
