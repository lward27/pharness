use std::time::{SystemTime, UNIX_EPOCH};

pub(in crate::app) fn unique_suffix() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

#[cfg(test)]
thread_local! {
    static TEST_OFFSET_MS: std::cell::Cell<i128> = const { std::cell::Cell::new(0) };
}

/// Move this thread's controller clock forward so bounded windows that span
/// minutes can be reconciled in a test without waiting for them. Each test
/// runs on its own thread, so the offset never leaks between tests.
#[cfg(test)]
pub(in crate::app) fn advance_for_test(ms: u64) {
    TEST_OFFSET_MS.with(|offset| offset.set(offset.get() + i128::from(ms)));
}

pub(in crate::app) fn current_millis() -> u128 {
    let real = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    #[cfg(test)]
    let real = (real as i128 + TEST_OFFSET_MS.with(std::cell::Cell::get)) as u128;
    real
}
