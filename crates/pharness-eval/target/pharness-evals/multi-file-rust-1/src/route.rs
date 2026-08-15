use crate::units::{meters, Kilometers};

pub fn route_length_meters(distance: Kilometers) -> u32 { meters(distance) }
