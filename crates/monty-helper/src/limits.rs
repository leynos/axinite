//! Resource ceilings the helper imposes on itself and on the guest.
//!
//! The parent cannot lower a child's limits without `unsafe` code, so the
//! helper applies them to itself before it reads any guest source. The
//! address-space ceiling is the last-resort boundary; the allocator budget and
//! the Monty tracker bound the guest far below it.

use std::io;
use std::time::Duration;

use monty_protocol::GuestLimits;
use monty_types::{ResourceLimits, memory_limit_with_headroom};

/// Total address space the helper may map, in bytes.
pub const ADDRESS_SPACE_LIMIT_BYTES: u64 = 512 * 1024 * 1024;

/// Largest guest heap the helper will arm, in bytes.
///
/// This is half the address-space ceiling, leaving room for the helper's own
/// allocations, the interpreter's metadata and allocator fragmentation.
pub const MAX_GUEST_MEMORY_BYTES: u64 = 256 * 1024 * 1024;

/// Applies the address-space ceiling to the current process.
///
/// The ceiling is the deployment contract for guest isolation, so a platform
/// that cannot impose it refuses to run rather than starting a guest with no
/// address-space bound: the plan forbids an unbounded platform fallback.
pub fn impose_address_space_limit() -> io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        use rustix::process::{Resource, Rlimit, setrlimit};
        setrlimit(
            Resource::As,
            Rlimit {
                current: Some(ADDRESS_SPACE_LIMIT_BYTES),
                maximum: Some(ADDRESS_SPACE_LIMIT_BYTES),
            },
        )?;
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "no address-space ceiling is available on this platform, so the \
             helper refuses to run a guest",
        ))
    }
}

/// Arms the `monty-alloc` hard ceiling for a guest heap budget.
pub fn arm_allocator(max_memory_bytes: u64) -> Result<(), String> {
    let memory = usize::try_from(max_memory_bytes)
        .map_err(|_| "guest memory budget does not fit this platform".to_owned())?;
    monty_alloc::set_hard_limit(memory_limit_with_headroom(Some(memory), false))
        .map_err(str::to_owned)
}

/// Refuses a guest heap budget above the helper's own ceiling.
pub fn check_guest_memory(max_memory_bytes: u64) -> Result<(), String> {
    if max_memory_bytes > MAX_GUEST_MEMORY_BYTES {
        return Err(format!(
            "max_memory_bytes {max_memory_bytes} exceeds the helper ceiling of {MAX_GUEST_MEMORY_BYTES}"
        ));
    }
    Ok(())
}

/// Translates the negotiated guest limits into Monty's tracker limits.
pub fn resource_limits(limits: &GuestLimits) -> ResourceLimits {
    ResourceLimits {
        max_feed_duration: Some(Duration::from_millis(limits.max_feed_millis)),
        max_turn_duration: Some(Duration::from_millis(limits.max_turn_millis)),
        max_memory: Some(to_usize(limits.max_memory_bytes)),
        gc_interval: None,
        max_recursion_depth: to_usize(limits.max_recursion_depth),
        max_suspensions: to_usize(limits.max_suspensions),
        max_total_sleep: None,
    }
}

fn to_usize(value: u64) -> usize {
    usize::try_from(value).unwrap_or(usize::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits() -> GuestLimits {
        GuestLimits {
            max_memory_bytes: 64 * 1024 * 1024,
            max_recursion_depth: 256,
            max_suspensions: 4096,
            max_feed_millis: 30_000,
            max_turn_millis: 1_500,
            max_output_bytes: 65_536,
        }
    }

    #[test]
    fn translates_each_negotiated_limit() {
        let limits = limits();
        let resource = resource_limits(&limits);
        assert_eq!(
            resource.max_feed_duration,
            Some(Duration::from_millis(30_000))
        );
        assert_eq!(
            resource.max_turn_duration,
            Some(Duration::from_millis(1_500))
        );
        assert_eq!(resource.max_memory, Some(64 * 1024 * 1024));
        assert_eq!(resource.max_recursion_depth, 256);
        assert_eq!(resource.max_suspensions, 4096);
    }

    #[test]
    fn refuses_a_guest_heap_above_the_helper_ceiling() {
        assert!(check_guest_memory(MAX_GUEST_MEMORY_BYTES).is_ok());
        assert!(check_guest_memory(MAX_GUEST_MEMORY_BYTES + 1).is_err());
    }
}
