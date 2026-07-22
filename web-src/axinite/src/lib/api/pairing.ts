import { postJson, requestJson } from "@/lib/api/client";
import type {
  ActionResponse,
  PairingApproveRequest,
  PairingListResponse,
} from "@/lib/api/contracts";

export function fetchPairingRequests(
  channel: string
): Promise<PairingListResponse> {
  return requestJson<PairingListResponse>(
    `/api/pairing/${encodeURIComponent(channel)}`
  );
}

export function approvePairing(
  channel: string,
  code: string
): Promise<ActionResponse> {
  const body: PairingApproveRequest = { code };
  // The gateway answers a rate-limited approval with 429 and a plain-text
  // body; `request` surfaces that body as the thrown Error's message, so the
  // caller can display it verbatim without special-casing the status here.
  return postJson<ActionResponse>(
    `/api/pairing/${encodeURIComponent(channel)}/approve`,
    body
  );
}
