/**
 * @file Opt-in simulated gateway downtime for exercising restart recovery.
 * This fixture never signals a process or restarts the daemon. It owns and
 * cancels its timers; HTTP and SSE adapters observe its availability state.
 */

/** Delay after the restart tool acknowledgement before the simulated outage. */
export const RESTART_OUTAGE_DELAY_MS = 650;
/** Five seconds spans a full three-second status poll interval. */
export const RESTART_OUTAGE_DURATION_MS = 5000;

type RestartFixturePhase = "idle" | "announced" | "unavailable" | "disposed";

/** Simulates an acknowledged restart, temporary outage, and recovery. */
export class RestartFixture {
  private phase: RestartFixturePhase = "idle";
  private outageTimer: ReturnType<typeof setTimeout> | undefined;
  private recoveryTimer: ReturnType<typeof setTimeout> | undefined;

  /** Builds a fixture with explicit activation and an adapter-owned stream closer. */
  constructor(
    readonly enabled: boolean,
    private readonly closeStreams: () => void
  ) {}

  /** Whether HTTP routes should currently respond with service unavailable. */
  get isUnavailable(): boolean {
    return this.phase === "unavailable";
  }

  /** Schedules one outage after acknowledgement; overlapping attempts are ignored. */
  acknowledge(): void {
    if (!this.enabled || this.phase !== "idle") {
      return;
    }
    this.phase = "announced";
    this.outageTimer = setTimeout(() => {
      this.outageTimer = undefined;
      this.phase = "unavailable";
      this.recoveryTimer = setTimeout(() => {
        this.recoveryTimer = undefined;
        this.phase = "idle";
      }, RESTART_OUTAGE_DURATION_MS);
      this.closeStreams();
    }, RESTART_OUTAGE_DELAY_MS);
  }

  /** Cancels all fixture timers and prevents subsequent restart attempts. */
  dispose(): void {
    clearTimeout(this.outageTimer);
    clearTimeout(this.recoveryTimer);
    this.outageTimer = undefined;
    this.recoveryTimer = undefined;
    this.phase = "disposed";
  }
}
