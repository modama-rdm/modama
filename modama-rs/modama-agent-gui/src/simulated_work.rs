use std::time::Duration;

/// Delay fixture requests in normal builds so their async states are visible.
/// Tests keep the same background-task boundaries without paying the delay.
#[cfg(not(any(test, feature = "visual-tests")))]
pub(crate) fn sleep(duration: Duration) {
    std::thread::sleep(duration);
}

#[cfg(any(test, feature = "visual-tests"))]
pub(crate) const fn sleep(_duration: Duration) {}
