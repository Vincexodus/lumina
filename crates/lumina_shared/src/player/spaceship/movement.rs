use avian2d::prelude::*;
use bevy::prelude::*;
use lightyear::prelude::*;
use lumina_common::prelude::*;

// For doc links
#[allow(unused_imports)]
use crate::action::PlayerAction;
use crate::player::PlayerId;
use crate::prelude::*;

use super::{Spaceship, SpaceshipAction};

pub(super) struct SpaceshipMovementPlugin;

impl Plugin for SpaceshipMovementPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            FixedUpdate,
            (
                (reset_spaceship_dynamics, target_direction),
                (
                    spaceship_rotation,
                    base_movement,
                    boost_movement,
                    dash_movement,
                    brake_movement,
                ),
                (apply_acceleration, dash_effect, dash_cooldown, regen_energy),
            )
                .chain()
                .after(super::spaceship_actions),
        )
        .add_systems(PreUpdate, init_energy);
    }
}

/// Initialize [`Energy::energy`] with [`EnergyConfig::max_energy`].
fn init_energy(
    mut q_spaceships: Query<(&mut Energy, &Spaceship), (Added<Energy>, With<SourceEntity>)>,
) {
    for (
        mut energy,
        Spaceship {
            energy: energy_config,
            ..
        },
    ) in q_spaceships.iter_mut()
    {
        energy.energy = energy_config.max_energy;
    }
}

/// Reset spaceship's [`TargetAcceleration`].
fn reset_spaceship_dynamics(mut q_spaceships: Query<&mut TargetAcceleration>) {
    for mut acceleration in q_spaceships.iter_mut() {
        **acceleration = 0.0;
    }
}

/// Determine the spaceship's [`TargetDirection`].
fn target_direction(
    mut q_spaceships: Query<
        (&mut TargetDirection, &SpaceshipAction, &Rotation),
        (With<Spaceship>, With<SourceEntity>),
    >,
) {
    for (mut direction, action, rotation) in q_spaceships.iter_mut() {
        **direction = action
            .movement_direction
            // Use existing rotation as the direction if there were no specific actions.
            .unwrap_or(Vec2::new(rotation.cos, rotation.sin));
    }
}

/// Rotate spaceships towards the [`TargetDirection`] using [`MovementConfig::rotation_speed`].
fn spaceship_rotation(
    mut q_spaceships: Query<
        (
            &mut Rotation,
            &mut RotationDiff,
            &TargetDirection,
            &Spaceship,
        ),
        With<SourceEntity>,
    >,
    time: Res<Time>,
) {
    for (mut rotation, mut rotation_diff, direction, Spaceship { movement, .. }) in
        q_spaceships.iter_mut()
    {
        let target_rotation = Rotation::from_sin_cos(direction.y, direction.x);
        let prev_rotation = rotation.as_radians();

        *rotation = rotation.slerp(
            target_rotation,
            time.delta_seconds() * movement.rotation_speed,
        );

        **rotation_diff = rotation.as_radians() - prev_rotation;
    }
}

/// Move spaceship from [`MovementConfig`] if [`PlayerAction::Move`] is being pressed.
fn base_movement(
    mut q_spaceships: Query<
        (&mut TargetAcceleration, &SpaceshipAction, &Spaceship),
        (Without<DashEffect>, With<SourceEntity>),
    >,
) {
    for (mut acceleration, action, spaceship) in q_spaceships.iter_mut() {
        if action.movement_direction.is_some() {
            **acceleration += spaceship.movement.linear_acceleration;
        }
    }
}

/// Consume [`Energy::energy`] and apply [`BoostConfig::linear_acceleration`] if [`PlayerAction::Boost`] is being pressed.
fn boost_movement(
    mut q_spaceships: Query<
        (
            &mut TargetAcceleration,
            &mut Energy,
            &SpaceshipAction,
            &Spaceship,
            Has<AbilityActive>,
            Has<ShadowAbilityConfig>,
        ),
        (Without<DashEffect>, With<SourceEntity>),
    >,
    time: Res<Time>,
) {
    for (
        mut acceleration,
        mut energy,
        action,
        Spaceship { boost, .. },
        ability_active,
        shadow_config,
    ) in q_spaceships.iter_mut()
    {
        let consumption = boost.energy_consumption * time.delta_seconds();

        // Check if ShadowAbility is active
        let is_shadow_active = ability_active && shadow_config;

        if action.is_boosting {
            if !is_shadow_active && energy.energy < consumption {
                // Skip if not enough energy and ShadowAbility is not active
                continue;
            }
            if !is_shadow_active {
                // Consume energy only if ShadowAbility is not active
                energy.energy -= consumption;
            }
            **acceleration += boost.linear_acceleration;
        }
    }
}

/// Apply damping from [`BrakeConfig`] based on [`PlayerAction::Brake`].
fn brake_movement(
    mut q_spaceships: Query<
        (&mut LinearDamping, &SpaceshipAction, &Spaceship),
        (Without<DashEffect>, With<SourceEntity>),
    >,
    time: Res<Time>,
) {
    const FACTOR: f32 = 4.0;

    for (mut damping, action, Spaceship { brake, .. }) in q_spaceships.iter_mut() {
        let target_damping = match action.is_braking {
            true => brake.brake_linear_damping,
            false => brake.linear_damping,
        };

        **damping = damping.lerp(target_damping, time.delta_seconds() * FACTOR);
    }
}

/// Consume [`Energy::energy`] and add [`DashEffect`] if [`PlayerAction::Dash`] is just being pressed.
fn dash_movement(
    mut commands: Commands,
    mut q_spaceships: Query<
        (
            &mut Energy,
            &TargetDirection,
            &SpaceshipAction,
            &Spaceship,
            Entity,
            Has<AbilityActive>,
            Has<ShadowAbilityConfig>,
        ),
        (
            Without<DashEffect>,
            Without<DashCooldown>,
            With<SourceEntity>,
        ),
    >,
) {
    for (
        mut energy,
        direction,
        action,
        Spaceship { dash, .. },
        entity,
        ability_active,
        shadow_config,
    ) in q_spaceships.iter_mut()
    {
        // Check if ShadowAbility is active
        let is_shadow_active = ability_active && shadow_config;

        if action.is_dash {
            if !is_shadow_active && energy.energy < dash.energy_consumption {
                // Skip if not enough energy and ShadowAbility is not active
                continue;
            }
            if !is_shadow_active {
                // Consume energy only if ShadowAbility is not active
                energy.energy -= dash.energy_consumption;
            }
            commands.entity(entity).insert((
                DashEffect {
                    timer: Timer::from_seconds(dash.duration, TimerMode::Once),
                    direction: **direction,
                },
                DashCooldown(Timer::from_seconds(dash.cooldown, TimerMode::Once)),
            ));
        }
    }
}

/// Apply [`TargetAcceleration`] towards the [`TargetDirection`] to [`LinearVelocity`].
fn apply_acceleration(
    mut q_accelerations: Query<
        (
            &mut LinearVelocity,
            &TargetAcceleration,
            &TargetDirection,
            &Spaceship,
        ),
        Without<DashEffect>,
    >,
    time: Res<Time>,
) {
    for (mut velocity, acceleration, direction, Spaceship { movement, .. }) in
        q_accelerations.iter_mut()
    {
        **velocity += **acceleration * **direction * time.delta_seconds();
        **velocity = velocity.clamp_length_max(movement.max_linear_speed);
    }
}

/// Overwrite velocity with [`DashConfig::impulse`] while [`DashEffect`] component is still active.
fn dash_effect(
    mut commands: Commands,
    mut q_spaceships: Query<
        (&mut LinearVelocity, &mut DashEffect, &Spaceship, Entity),
        With<SourceEntity>,
    >,
    time: Res<Time>,
) {
    for (mut velocity, mut dash_timer, Spaceship { dash, .. }, entity) in q_spaceships.iter_mut() {
        dash_timer.timer.tick(time.delta());

        **velocity = dash_timer.direction * dash.impulse;

        if dash_timer.timer.finished() {
            commands.entity(entity).remove::<DashEffect>();
        }
    }
}

fn dash_cooldown(
    mut commands: Commands,
    mut q_cooldowns: Query<(&mut DashCooldown, &PlayerId, Entity), With<SourceEntity>>,
    time: Res<Time>,
    network_identity: NetworkIdentity,
) {
    for (mut cooldown, player_id, entity) in q_cooldowns.iter_mut() {
        if cooldown.tick(time.delta()).finished()
            && (network_identity.is_server() || player_id.is_local())
        {
            commands.entity(entity).remove::<DashCooldown>();
        }
    }
}

fn regen_energy(
    mut q_spaceships: Query<(&mut Energy, &Spaceship), With<SourceEntity>>,
    time: Res<Time>,
) {
    for (
        mut energy,
        Spaceship {
            energy: energy_config,
            ..
        },
    ) in q_spaceships.iter_mut()
    {
        if energy.energy < energy.prev_energy {
            // Energy is being used since last frame.
            energy.cooldown = energy_config.cooldown;
        } else {
            energy.cooldown -= time.delta_seconds();
            energy.cooldown = energy.cooldown.max(0.0);
        }

        // Regenerate energy once cooldown is reached.
        if energy.cooldown <= 0.0 {
            // Just in case that the energy somehow went below 0.0.
            energy.energy = energy.energy.max(0.0);
            energy.energy += energy_config.regen_rate * time.delta_seconds();
            energy.energy = energy.energy.min(energy_config.max_energy);
        }

        // Update previous energy.
        energy.prev_energy = energy.energy;
    }
}

/// Flow of spaceship movement:
/// 1. Collect spaceship actions into [`SpaceshipAction`].
/// 2. Determine the spaceship's [`TargetDirection`].
/// 3. Determine base velocity if [`PlayerAction::Move`] is pressed.
///   - Write to [`TargetAcceleration`].
/// 4. Determine damping value based on [`PlayerAction::Brake`].
///   - Write to [`TargetDamping`].
/// 5. Apply impulse velocity if [`PlayerAction::Dash`] is pressed.
///   - Add to [`TargetAcceleration`].
#[derive(Bundle, Default)]
pub struct SpaceshipMovementBundle {
    pub direction: TargetDirection,
    pub speed: TargetAcceleration,
    pub damping: TargetDamping,
    pub rotation_diff: RotationDiff,
    pub energy: Energy,
}

#[derive(Component, Deref, DerefMut, Default, Debug, Clone, Copy, PartialEq)]
pub struct TargetDirection(pub Vec2);

#[derive(Component, Deref, DerefMut, Default, Debug, Clone, Copy, PartialEq)]
pub struct TargetAcceleration(pub f32);

#[derive(Component, Deref, DerefMut, Default, Debug, Clone, Copy, PartialEq)]
pub struct TargetDamping(pub f32);

#[derive(Component, Deref, DerefMut, Default, Debug, Clone, Copy, PartialEq)]
pub struct RotationDiff(pub f32);

/// Apply dasing from [`DashConfig`] while this component is still in effect.
/// This component will be removed when timer ends.
#[derive(Component, Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct DashEffect {
    /// Time until the dash effect finishes.
    pub timer: Timer,
    /// The dash direction when [PlayerAction::Dash] is just being pressed.
    pub direction: Vec2,
}

/// Cooldown timer based on [`DashConfig::cooldown`].
/// While this component is still in effect, [dash action][crate::action::PlayerAction::Dash] cannot be used.
#[derive(Component, Serialize, Deserialize, Deref, DerefMut, Debug, Clone, PartialEq)]
pub struct DashCooldown(Timer);

#[derive(Component, Serialize, Deserialize, Default, Debug, Clone, Copy, PartialEq)]
pub struct Energy {
    /// Previous energy level.
    prev_energy: f32,
    /// Current energy level.
    pub energy: f32,
    /// Remaining cooldown time.
    pub cooldown: f32,
}

#[derive(Reflect, Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub struct MovementConfig {
    /// Normal linear acceleration.
    pub linear_acceleration: f32,
    /// Maximum magnitude of the linear velocity.
    pub max_linear_speed: f32,
    /// Rotation speed of the spaceship.
    pub rotation_speed: f32,
}

#[derive(Reflect, Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub struct BrakeConfig {
    /// Normal linear damping.
    pub linear_damping: f32,
    /// Brake linear damping.
    pub brake_linear_damping: f32,
}

#[derive(Reflect, Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub struct BoostConfig {
    /// Boost linear acceleration.
    pub linear_acceleration: f32,
    /// Energy consumption rate.
    pub energy_consumption: f32,
}

#[derive(Reflect, Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub struct DashConfig {
    // How long the dash lasts (seconds).
    pub duration: f32,
    // Cooldown after dash (seconds).
    pub cooldown: f32,
    // Energy consumption for a single use.
    pub energy_consumption: f32,
    // Dash impulse force.
    pub impulse: f32,
}

#[derive(Reflect, Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub struct EnergyConfig {
    // Maximum energy level.
    pub max_energy: f32,
    // Energy regeneration rate.
    pub regen_rate: f32,
    // Cooldown duration in seconds before energy regenerates.
    pub cooldown: f32,
}
