export type FeatureFlagName =
  | "route_chat"
  | "route_memory"
  | "route_jobs"
  | "route_routines"
  | "route_extensions"
  | "route_skills"
  | "panel_logs"
  | "action_memory_edit"
  | "action_job_restart"
  | "action_routine_trigger"
  | "action_extension_install"
  | "action_skill_install"
  | "surface_tee_attestation";

export type FeatureFlagDefinition = {
  name: FeatureFlagName;
  defaultValue: boolean;
  owner: string;
  backendContract: string;
};

export const FEATURE_FLAGS: FeatureFlagDefinition[] = [
  {
    name: "route_chat",
    defaultValue: true,
    owner: "chat-ui",
    backendContract: "always available in preview shell",
  },
  {
    name: "route_memory",
    defaultValue: true,
    owner: "memory-ui",
    backendContract: "always available in preview shell",
  },
  {
    name: "route_jobs",
    defaultValue: true,
    owner: "jobs-ui",
    backendContract: "hide when jobs runtime is absent",
  },
  {
    name: "route_routines",
    defaultValue: true,
    owner: "routines-ui",
    backendContract: "hide when scheduler runtime is absent",
  },
  {
    name: "route_extensions",
    defaultValue: true,
    owner: "extensions-ui",
    backendContract: "hide when extension runtime is absent",
  },
  {
    name: "route_skills",
    defaultValue: true,
    owner: "skills-ui",
    backendContract: "hide when skill registry is absent",
  },
  {
    name: "panel_logs",
    defaultValue: true,
    owner: "shell-ui",
    backendContract: "hide when log stream is absent",
  },
  {
    name: "action_memory_edit",
    defaultValue: false,
    owner: "memory-ui",
    backendContract: "enable only after write API is stable",
  },
  {
    name: "action_job_restart",
    defaultValue: false,
    owner: "jobs-ui",
    backendContract: "enable only after restart endpoint is stable",
  },
  {
    name: "action_routine_trigger",
    defaultValue: false,
    owner: "routines-ui",
    backendContract: "enable only after trigger endpoint is stable",
  },
  {
    name: "action_extension_install",
    defaultValue: false,
    owner: "extensions-ui",
    backendContract: "enable only after install endpoint is stable",
  },
  {
    name: "action_skill_install",
    defaultValue: false,
    owner: "skills-ui",
    backendContract: "enable only after catalogue endpoints are stable",
  },
  {
    name: "surface_tee_attestation",
    defaultValue: false,
    owner: "shell-ui",
    backendContract: "enable only when attestation data is available",
  },
];

export function getFeatureFlagDefaults(): Record<FeatureFlagName, boolean> {
  return FEATURE_FLAGS.reduce(
    (accumulator, flag) => {
      accumulator[flag.name] = flag.defaultValue;
      return accumulator;
    },
    {} as Record<FeatureFlagName, boolean>
  );
}
