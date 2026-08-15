pub fn is_even(value: u32) -> bool { value % 2 == 0 }

#[cfg(test)]
mod tests { use super::is_even; #[test] fn recognizes_even_values() { assert!(is_even(4)); } }
