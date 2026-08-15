pub mod units;
pub mod route;

#[cfg(test)]
mod tests { use crate::{route::route_length_meters, units::Kilometers}; #[test] fn converts_route_distance() { assert_eq!(route_length_meters(Kilometers(2)), 2000); } }
