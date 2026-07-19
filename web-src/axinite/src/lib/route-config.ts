export type RouteId =
  | "chat"
  | "memory"
  | "jobs"
  | "routines"
  | "extensions"
  | "skills";

const SHARED_CARD_KEYS = ["card-a", "card-b", "card-c"] as const;

type RouteDetails = {
  flagName: `route_${RouteId}`;
  cardKeys: typeof SHARED_CARD_KEYS;
};

export const ROUTE_ORDER: RouteId[] = [
  "chat",
  "memory",
  "jobs",
  "routines",
  "extensions",
  "skills",
];

export const ROUTE_DETAILS: Record<RouteId, RouteDetails> = {
  chat: {
    flagName: "route_chat",
    cardKeys: SHARED_CARD_KEYS,
  },
  memory: {
    flagName: "route_memory",
    cardKeys: SHARED_CARD_KEYS,
  },
  jobs: {
    flagName: "route_jobs",
    cardKeys: SHARED_CARD_KEYS,
  },
  routines: {
    flagName: "route_routines",
    cardKeys: SHARED_CARD_KEYS,
  },
  extensions: {
    flagName: "route_extensions",
    cardKeys: SHARED_CARD_KEYS,
  },
  skills: {
    flagName: "route_skills",
    cardKeys: SHARED_CARD_KEYS,
  },
};
