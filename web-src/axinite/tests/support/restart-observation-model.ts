/** @file Restart test oracle records witnesses independently of controller callbacks. */
import type { RestartPhase } from "@/lib/restart";

type Observation = "ack" | "down" | "up";

// The oracle records meaningful observations rather than copying controller
// callbacks or cleanup fields. Completion needs an ordered ack/down/up witness.
/** Independent ordered-observation oracle for bounded restart histories. */
export class ObservationModel {
  readonly attempts: Observation[][] = [];
  readonly phases: RestartPhase[] = [];
  active: number | undefined;
  closed = false;
  pollsStarted = 0;
  timersStarted = 0;

  start(): void {
    if (this.closed || this.active !== undefined) return;
    this.active = this.attempts.length;
    this.attempts.push([]);
    this.phases.push("restarting");
  }

  private recordAcknowledgement(history: Observation[]): void {
    if (history.includes("ack")) return;
    history.push("ack");
    this.timersStarted += 1;
  }

  private hasRecoveryWitness(history: Observation[]): boolean {
    const down = history.indexOf("down");
    return down !== -1 && history.slice(down + 1).includes("up");
  }

  /** Accept live observations and complete only after an ordered ack/down/up witness. */
  observe(owner: number, observation: Observation): void {
    if (this.closed || this.active !== owner) return;
    const history = this.attempts[owner];
    if (!history) throw new Error("Missing model attempt");
    if (observation === "ack") {
      this.recordAcknowledgement(history);
      return;
    }
    if (!history.includes("ack")) return;
    history.push(observation);
    if (this.hasRecoveryWitness(history)) {
      this.closed = true;
      this.active = undefined;
      this.phases.push("restarted");
    }
  }

  fail(owner: number): void {
    if (this.closed || this.active !== owner) return;
    this.active = undefined;
    this.phases.push("failed");
  }

  dispose(): void {
    this.closed = true;
    this.active = undefined;
  }
}
