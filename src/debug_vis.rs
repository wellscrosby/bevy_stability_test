use bevy::{
    diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin},
    ecs::system::SystemParam,
    prelude::*,
    text::{TextColor, TextFont},
    ui::Node,
};
use std::{
    collections::{HashMap, VecDeque},
    sync::{Mutex, OnceLock},
};

const LINE_HEIGHT: f32 = 20.0;
const LEFT_PADDING: f32 = 12.0;
const FRAME_DELTA_WINDOW: usize = 300;
const FPS_AVG_WINDOW_SECONDS: f64 = 0.25;

pub struct DebugVisPlugin;

#[derive(Default, Reflect, GizmoConfigGroup)]
struct DebugTopGizmoGroup;

#[derive(Resource, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebugLevel {
    Hidden,
    FpsOnly,
    #[default]
    Full,
}

impl Plugin for DebugVisPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DebugTexts>()
            .init_resource::<DebugLevel>()
            .init_resource::<FrameTimeHistory>()
            .init_gizmo_group::<DebugTopGizmoGroup>()
            .add_plugins(FrameTimeDiagnosticsPlugin::default())
            .add_systems(Startup, (spawn_fps_display, setup_debug_top_gizmo_config))
            .add_systems(
                Update,
                (
                    update_frame_time_history,
                    update_fps_display,
                    update_frametime_consistency_display.after(update_frame_time_history),
                    drain_debug_queue,
                    cleanup_stale_debug_texts,
                    // toggle_debug_level,
                    apply_debug_visibility,
                ),
            )
            .add_systems(
                PostUpdate,
                (
                    draw_axes_gizmo.after(TransformSystems::Propagate),
                    draw_frametime_barchart.after(TransformSystems::Propagate),
                ),
            );
    }
}

#[derive(Resource, Default)]
struct DebugTexts {
    frame: u64,
    next_line: usize,
    line_lookup: HashMap<String, usize>,
    entries: HashMap<String, DebugEntry>,
}

struct DebugEntry {
    entity: Entity,
    line: usize,
    last_frame: u64,
    persistent: bool,
}

#[derive(Component)]
struct DebugLabel(String);

/// System param helper to write/update debug text lines.
#[derive(SystemParam)]
pub struct DebugTextWriter<'w, 's> {
    commands: Commands<'w, 's>,
    texts: ResMut<'w, DebugTexts>,
    level: Res<'w, DebugLevel>,
}

impl<'w, 's> DebugTextWriter<'w, 's> {
    pub fn write(&mut self, key: impl Into<String>, message: impl Into<String>) {
        self.write_with_persistence(key, message, false);
    }

    pub fn write_with_persistence(
        &mut self,
        key: impl Into<String>,
        message: impl Into<String>,
        persistent: bool,
    ) {
        let key = key.into();
        let message = message.into();
        let frame = self.texts.frame;

        if let Some(entry) = self.texts.entries.get_mut(&key) {
            self.commands
                .entity(entry.entity)
                .insert(Text::new(message));
            entry.last_frame = frame;
            entry.persistent |= persistent;
        } else {
            let line = if let Some(line) = self.texts.line_lookup.get(&key) {
                *line
            } else {
                let line = self.texts.next_line;
                self.texts.next_line += 1;
                self.texts.line_lookup.insert(key.clone(), line);
                line
            };

            let visibility = if *self.level == DebugLevel::Full {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };

            let entity = self
                .commands
                .spawn((
                    DebugLabel(key.clone()),
                    Text::new(message),
                    TextFont {
                        font_size: 16.0,
                        ..default()
                    },
                    TextColor(Color::srgb(0.0, 1.0, 0.0)),
                    TextShadow{
                        offset: Vec2::new(1.0, 1.0),
                        color: Color::srgb(0.0, 0.0, 0.0),
                    },
                    Node {
                        position_type: PositionType::Absolute,
                        bottom: Val::Px(line as f32 * LINE_HEIGHT),
                        left: Val::Px(LEFT_PADDING),
                        ..default()
                    },
                    visibility,
                ))
                .id();

            self.texts.entries.insert(
                key,
                DebugEntry {
                    entity,
                    line,
                    last_frame: frame,
                    persistent,
                },
            );
        }
    }
}

#[derive(Component)]
struct FpsText;

#[derive(Component)]
struct FrametimeConsistencyText;

#[derive(Component)]
struct FrametimeMaxDeltaText;

#[derive(Resource, Default)]
struct FrameTimeHistory {
    frame_times_ms: VecDeque<f64>,
    sum_seconds: f64,
}

fn spawn_fps_display(mut commands: Commands, level: Res<DebugLevel>) {
    let visibility = if *level == DebugLevel::Hidden {
        Visibility::Hidden
    } else {
        Visibility::Inherited
    };

    commands.spawn((
        FpsText,
        Text::new("FPS: --"),
        TextFont {
            font_size: 16.0,
            ..default()
        },
        TextColor(Color::srgb(0.0, 1.0, 0.0)),
        TextShadow{
            offset: Vec2::new(1.0, 1.0),
            color: Color::srgb(0.0, 0.0, 0.0),
        },
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(8.0),
            left: Val::Px(8.0),
            ..default()
        },
        visibility,
    ));

    let consistency_visibility = if *level == DebugLevel::Full {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };

    commands.spawn((
        FrametimeConsistencyText,
        Text::new(format!("Frametime avg ({}): --", FRAME_DELTA_WINDOW)),
        TextFont {
            font_size: 16.0,
            ..default()
        },
        TextColor(Color::srgb(0.0, 1.0, 0.0)),
        TextShadow {
            offset: Vec2::new(1.0, 1.0),
            color: Color::srgb(0.0, 0.0, 0.0),
        },
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(28.0),
            left: Val::Px(8.0),
            ..default()
        },
        consistency_visibility,
    ));

    commands.spawn((
        FrametimeMaxDeltaText,
        Text::new(format!("Frametime max ({}): --", FRAME_DELTA_WINDOW)),
        TextFont {
            font_size: 16.0,
            ..default()
        },
        TextColor(Color::srgb(0.0, 1.0, 0.0)),
        TextShadow {
            offset: Vec2::new(1.0, 1.0),
            color: Color::srgb(0.0, 0.0, 0.0),
        },
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(48.0),
            left: Val::Px(8.0),
            ..default()
        },
        consistency_visibility,
    ));
}

fn update_frame_time_history(
    diagnostics: Res<DiagnosticsStore>,
    mut history: ResMut<FrameTimeHistory>,
) {
    let Some(frame_time_ms) = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FRAME_TIME)
        .and_then(|d| d.smoothed())
    else {
        return;
    };

    history.frame_times_ms.push_back(frame_time_ms);
    history.sum_seconds += frame_time_ms / 1000.0;
    if history.frame_times_ms.len() > FRAME_DELTA_WINDOW {
        if let Some(removed) = history.frame_times_ms.pop_front() {
            history.sum_seconds -= removed / 1000.0;
        }
    }
}

fn update_fps_display(
    level: Res<DebugLevel>,
    history: Res<FrameTimeHistory>,
    mut query: Query<&mut Text, With<FpsText>>,
) {
    if *level == DebugLevel::Hidden {
        return;
    }

    let Ok(mut text) = query.single_mut() else {
        return;
    };

    let mut window_seconds = 0.0;
    let mut frames = 0usize;
    for frame_time_ms in history.frame_times_ms.iter().rev() {
        window_seconds += frame_time_ms / 1000.0;
        frames += 1;
        if window_seconds >= FPS_AVG_WINDOW_SECONDS {
            break;
        }
    }

    if window_seconds > 0.0 {
        let fps = frames as f64 / window_seconds;
        text.0 = format!("FPS: {:.0}", fps);
    }
}

fn update_frametime_consistency_display(
    level: Res<DebugLevel>,
    history: Res<FrameTimeHistory>,
    mut text_queries: ParamSet<(
        Query<&mut Text, (With<FrametimeConsistencyText>, Without<FpsText>)>,
        Query<&mut Text, (With<FrametimeMaxDeltaText>, Without<FpsText>)>,
    )>,
) {
    if *level != DebugLevel::Full {
        return;
    }

    let (avg_label, max_label) = {
        if history.frame_times_ms.is_empty() {
            (
                format!("Frametime avg ({}): --", FRAME_DELTA_WINDOW),
                format!("Frametime max ({}): --", FRAME_DELTA_WINDOW),
            )
        } else {
            let avg = (history.sum_seconds * 1000.0) / history.frame_times_ms.len() as f64;
            let max_frame_time = history
                .frame_times_ms
                .iter()
                .copied()
                .fold(0.0_f64, f64::max);
            (
                format!("Frametime avg ({}): {:.2}", FRAME_DELTA_WINDOW, avg),
                format!("Frametime max ({}): {:.2}", FRAME_DELTA_WINDOW, max_frame_time),
            )
        }
    };

    let mut avg_query = text_queries.p0();
    let Ok(mut avg_text) = avg_query.single_mut() else {
        return;
    };
    avg_text.0 = avg_label;

    let mut max_query = text_queries.p1();
    let Ok(mut max_text) = max_query.single_mut() else {
        return;
    };
    max_text.0 = max_label;
}

// fn toggle_debug_level(
//     mut debug_reader: MessageReader<DebugAction>,
//     mut level: ResMut<DebugLevel>,
// ) {
//     for event in debug_reader.read() {
//         match event {
//             DebugAction::ToggleDebugLevel => {
//                 *level = match *level {
//                     DebugLevel::Hidden => DebugLevel::FpsOnly,
//                     DebugLevel::FpsOnly => DebugLevel::Full,
//                     DebugLevel::Full => DebugLevel::Hidden,
//                 };
//             }
//         }
//     }
// }

fn apply_debug_visibility(
    level: Res<DebugLevel>,
    mut fps_query: Query<&mut Visibility, (With<FpsText>, Without<FrametimeConsistencyText>)>,
    mut consistency_query: Query<&mut Visibility, (With<FrametimeConsistencyText>, Without<FpsText>)>,
    mut debug_query: Query<
        &mut Visibility,
        (
            With<DebugLabel>,
            Without<FpsText>,
            Without<FrametimeConsistencyText>,
        ),
    >,
) {
    if !level.is_changed() {
        return;
    }

    let (fps_vis, consistency_vis, debug_vis) = match *level {
        DebugLevel::Hidden => (Visibility::Hidden, Visibility::Hidden, Visibility::Hidden),
        DebugLevel::FpsOnly => (Visibility::Inherited, Visibility::Hidden, Visibility::Hidden),
        DebugLevel::Full => (
            Visibility::Inherited,
            Visibility::Inherited,
            Visibility::Inherited,
        ),
    };

    for mut vis in fps_query.iter_mut() {
        if *vis != fps_vis {
            *vis = fps_vis;
        }
    }
    for mut vis in consistency_query.iter_mut() {
        if *vis != consistency_vis {
            *vis = consistency_vis;
        }
    }
    for mut vis in debug_query.iter_mut() {
        if *vis != debug_vis {
            *vis = debug_vis;
        }
    }
}

pub fn debug_text(key: impl Into<String>, message: impl Into<String>) {
    enqueue_request(DebugRequest {
        key: key.into(),
        message: message.into(),
        persistent: false,
    });
}

pub fn debug_text_persistent(key: impl Into<String>, message: impl Into<String>) {
    enqueue_request(DebugRequest {
        key: key.into(),
        message: message.into(),
        persistent: true,
    });
}

struct DebugRequest {
    key: String,
    message: String,
    persistent: bool,
}

static DEBUG_QUEUE: OnceLock<Mutex<Vec<DebugRequest>>> = OnceLock::new();

fn enqueue_request(req: DebugRequest) {
    if let Ok(mut queue) = DEBUG_QUEUE.get_or_init(|| Mutex::new(Vec::new())).lock() {
        queue.push(req);
    }
}

fn drain_debug_queue(mut writer: DebugTextWriter) {
    let Some(queue) = DEBUG_QUEUE.get() else {
        return;
    };
    let mut queue = queue.lock().unwrap();
    for req in queue.drain(..) {
        writer.write_with_persistence(req.key, req.message, req.persistent);
    }
}

fn cleanup_stale_debug_texts(mut texts: ResMut<DebugTexts>, mut commands: Commands) {
    texts.frame = texts.frame.wrapping_add(1);
    let frame = texts.frame;

    let mut to_remove = Vec::new();
    for (key, entry) in texts.entries.iter() {
        if !entry.persistent && entry.last_frame + 1 < frame {
            commands.entity(entry.entity).despawn();
            to_remove.push(key.clone());
        }
    }

    for key in to_remove {
        texts.entries.remove(&key);
    }
}

fn setup_debug_top_gizmo_config(mut config_store: ResMut<GizmoConfigStore>) {
    let (config, _) = config_store.config_mut::<DebugTopGizmoGroup>();
    config.depth_bias = -1.0;
}

fn draw_axes_gizmo(
    level: Res<DebugLevel>,
    camera_query: Query<(&Camera, &GlobalTransform)>,
    mut gizmos: Gizmos<DebugTopGizmoGroup>,
) {
    if *level != DebugLevel::Full {
        return;
    }

    let Ok((camera, camera_transform)) = camera_query.single() else {
        return;
    };

    // We want the axes to appear in the top-left, below the FPS.
    // The FPS is at (8, 8). Let's put the axes center at roughly (50, 100) in screen pixels.
    let axes_screen_pos = Vec2::new(100.0, 170.0);

    // Convert screen position to world position at a fixed distance from the camera.
    let Ok(ray) = camera.viewport_to_world(camera_transform, axes_screen_pos) else {
        return;
    };

    let world_pos: Vec3 = ray.get_point(0.5); // 0.5 units in front of the camera

    // Draw axes at that world position, but orientation should be absolute (world axes)
    let length = 0.03;

    // World X - Red
    gizmos.line(
        world_pos,
        world_pos + Vec3::X * length,
        Color::srgb(1.0, 0.0, 0.0),
    );
    // World Y - Green
    gizmos.line(
        world_pos,
        world_pos + Vec3::Y * length,
        Color::srgb(0.0, 1.0, 0.0),
    );
    // World Z - Blue
    gizmos.line(
        world_pos,
        world_pos + Vec3::Z * length,
        Color::srgb(0.0, 0.0, 1.0),
    );
}

fn draw_frametime_barchart(
    level: Res<DebugLevel>,
    history: Res<FrameTimeHistory>,
    camera_query: Query<(&Camera, &GlobalTransform)>,
    mut gizmos: Gizmos<DebugTopGizmoGroup>,
) {
    if *level != DebugLevel::Full {
        return;
    }

    let Ok((camera, camera_transform)) = camera_query.single() else {
        return;
    };

    let max_ms = history
        .frame_times_ms
        .iter()
        .copied()
        .fold(0.0_f64, f64::max);

    let min_ms = history
        .frame_times_ms
        .iter()
        .copied()
        .fold(f64::INFINITY, f64::min);

    let avg_ms = history.sum_seconds * 1000.0 / history.frame_times_ms.len() as f64;

    let chart_origin = Vec2::new(8.0, 120.0);
    let chart_width = 300.0;
    let bar_width = chart_width / history.frame_times_ms.len() as f32;
    let max_height = 50.0;
    let depth = 0.5;

    let start_index = history
        .frame_times_ms
        .len()
        .saturating_sub(FRAME_DELTA_WINDOW);
    for (idx, frame_time) in history.frame_times_ms.iter().skip(start_index).enumerate() {
        let color_ratio = if *frame_time > avg_ms { 0.2 + ((*frame_time / avg_ms - 1.0).clamp(0.0, 1.0) * 0.8) } else { (*frame_time / avg_ms) * 0.2}; // an avg frame time is 20% red, a 2X avg frametime is 100% red
        let ratio = (*frame_time / max_ms).clamp(0.0, 1.0) as f32;
        let height = max_height * ratio;
        let x = chart_origin.x + idx as f32 * (bar_width);
        let base = Vec2::new(x, chart_origin.y);
        let top = Vec2::new(x, chart_origin.y - height);

        let Ok(base_ray) = camera.viewport_to_world(camera_transform, base) else {
            continue;
        };
        let Ok(top_ray) = camera.viewport_to_world(camera_transform, top) else {
            continue;
        };

        let base_pos = base_ray.get_point(depth);
        let top_pos = top_ray.get_point(depth);
        let color = Color::srgb(color_ratio as f32, 1.0 - color_ratio as f32, 0.0);

        gizmos.line(base_pos, top_pos, color);
    }
}
