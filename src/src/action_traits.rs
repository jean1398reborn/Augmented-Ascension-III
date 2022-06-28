use crate::char::{AvailableMovementActions, MovementAction};
use crate::VelocityForceCap;
use bevy::prelude::Vec3;


// Helper trait to assist me in scalign down certain types.
pub trait ScaleToRatio {
    fn scale_to_ratio(&self, ratio: &Vec3) -> Self;
}

// all of these types are basically just here as a basis so i can eventually scale up the AvailableMovementActions.
impl ScaleToRatio for Option<f32> {
    fn scale_to_ratio(&self, ratio: &Vec3) -> Self {
        match self {
            Some(f) => Some(f / ratio.y),
            None => None,
        }
    }
}

// Scaling this type is important since it is featured in the movement actions.
impl ScaleToRatio for Option<[f32; 2]> {
    fn scale_to_ratio(&self, ratio: &Vec3) -> Self {
        match self {
            Some(f) => Some([f[0] / ratio.y, f[1] / ratio.y]),
            None => None,
        }
    }
}


impl ScaleToRatio for VelocityForceCap {
    fn scale_to_ratio(&self, ratio: &Vec3) -> Self {
        VelocityForceCap {
            max_velocity: self.max_velocity.scale_to_ratio(ratio),
            max_angvel: self.max_angvel.scale_to_ratio(ratio),
            max_external_force: self.max_external_force.scale_to_ratio(ratio),
            max_external_torque: self.max_external_torque.scale_to_ratio(ratio),
        }
    }
}

impl ScaleToRatio for MovementAction {
    fn scale_to_ratio(&self, ratio: &Vec3) -> Self {
        Self {
            set_velocity: self.set_velocity.scale_to_ratio(ratio),
            add_velocity: self.add_velocity.scale_to_ratio(ratio),
            set_angvel: self.set_angvel.scale_to_ratio(ratio),
            add_angvel: self.add_angvel.scale_to_ratio(ratio),
            set_external_force: self.set_external_force.scale_to_ratio(ratio),
            add_external_force: self.add_external_force.scale_to_ratio(ratio),
            set_external_torque: self.set_external_torque.scale_to_ratio(ratio),
            add_external_torque: self.add_external_torque.scale_to_ratio(ratio),
            set_lin_damping: self.set_lin_damping.scale_to_ratio(ratio),
            add_lin_damping: self.add_lin_damping.scale_to_ratio(ratio),
            set_ang_damping: self.set_ang_damping.scale_to_ratio(ratio),
            add_ang_damping: self.add_ang_damping.scale_to_ratio(ratio),
            ..self.clone()
        }
    }
}

impl ScaleToRatio for AvailableMovementActions {
    fn scale_to_ratio(&self, ratio: &Vec3) -> Self {
        Self {

		// Scale each one of the possible movement actions
            pressed_up: self.pressed_up.scale_to_ratio(ratio),
            pressed_down: self.pressed_down.scale_to_ratio(ratio),
            pressed_left: self.pressed_left.scale_to_ratio(ratio),
            pressed_right: self.pressed_right.scale_to_ratio(ratio),
            unpressed_up: self.unpressed_up.scale_to_ratio(ratio),
            unpressed_down: self.unpressed_down.scale_to_ratio(ratio),
            unpressed_left: self.unpressed_left.scale_to_ratio(ratio),
            unpressed_right: self.unpressed_right.scale_to_ratio(ratio),
        }
    }
}
