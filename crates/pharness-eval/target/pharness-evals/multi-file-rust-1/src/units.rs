#[derive(Clone, Copy)]
pub struct Kilometers(pub u32);

pub fn meters(value: Kilometers) -> u32 { value.0 * 1_000 }
