//! macOS bridges for meeting notes. See docs/MEETING_NOTES_ARCH.md.

/// True on macOS 14.2+ (Core Audio process taps).
pub fn is_supported() -> bool {
    false
}
