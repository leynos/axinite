//! Disable-only subsystem policy; the gateway adapter supplies availability facts.

/// Availability of the optional subsystems supporting browser surfaces.
#[derive(Debug, Default)]
pub struct SubsystemAvailability {
    /// Jobs runtime or scheduler is present.
    pub jobs: bool,
    /// Routine engine is present.
    pub routines: bool,
    /// Extension manager is present.
    pub extensions: bool,
    /// Skill registry is present.
    pub skills: bool,
    /// Log broadcaster is present.
    pub logs: bool,
}

impl SubsystemAvailability {
    /// Flags forced off by absent subsystems, below the explicit override layers.
    pub fn unavailable_flags(&self) -> Vec<&'static str> {
        [
            (self.jobs, ["route_jobs", "action_job_restart"]),
            (self.routines, ["route_routines", "action_routine_trigger"]),
            (
                self.extensions,
                ["route_extensions", "action_extension_install"],
            ),
            (self.skills, ["route_skills", "action_skill_install"]),
            (self.logs, ["route_logs", "panel_logs"]),
        ]
        .into_iter()
        .filter(|(is_available, _)| !is_available)
        .flat_map(|(_, flags)| flags)
        .collect()
    }
}
