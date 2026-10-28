/** @file Bounded properties for Vite output framing and readiness recognition. */
import fc from "fast-check";
import { describe, expect, it, vi } from "vitest";
import { createBuildReadiness, pipeOutput } from "../../scripts/build-output";

function expectedLines(source: string) {
  const observed: string[] = [];
  let remainder = source;
  for (;;) {
    const end = remainder.indexOf("\n");
    if (end === -1) break;
    let line = remainder.slice(0, end);
    if (line.endsWith("\r")) line = line.slice(0, -1);
    observed.push(line);
    remainder = remainder.slice(end + 1);
  }
  return {
    observed,
    written: [...observed, ...(remainder ? [remainder] : [])].map(
      (line) => `[build] ${line}`
    ),
  };
}

const sourceText = fc
  .array(fc.constantFrom("a", "z", "é", "漢", "🚀", "\n", "\r"), {
    maxLength: 40,
  })
  .map((characters) => characters.join(""));
const chunkSizes = fc.array(fc.integer({ min: 1, max: 8 }), {
  minLength: 1,
  maxLength: 12,
});
const wholeNumbers = fc.nat({ max: 99_999 });
const fractionalNumbers = fc.option(fc.nat({ max: 999 }), { nil: undefined });

describe("build output properties", () => {
  it("frames UTF-8 output independently of byte chunk boundaries", async () => {
    await fc.assert(
      fc.asyncProperty(sourceText, chunkSizes, async (source, sizes) => {
        const bytes = new TextEncoder().encode(source);
        const stream = new TransformStream<Uint8Array, Uint8Array>();
        const writer = stream.writable.getWriter();
        const written = vi.fn();
        const observed = vi.fn();
        pipeOutput("build", stream.readable, written, observed);
        let offset = 0;
        let chunk = 0;
        while (offset < bytes.length) {
          const size = sizes[chunk % sizes.length] ?? 1;
          await writer.write(bytes.slice(offset, offset + size));
          offset += size;
          chunk += 1;
        }
        await writer.close();
        const expected = expectedLines(source);
        if (expected.written.length > 0) {
          await vi.waitFor(
            () =>
              expect(written).toHaveBeenCalledTimes(expected.written.length),
            { timeout: 500 }
          );
        }
        expect(written.mock.calls.map(([line]) => line)).toEqual(
          expected.written
        );
        expect(observed.mock.calls.map(([line]) => line)).toEqual(
          expected.observed
        );
      }),
      { numRuns: 80 }
    );
  });

  it("recognizes complete durations and rejects generated near misses", async () => {
    await fc.assert(
      fc.asyncProperty(
        wholeNumbers,
        fractionalNumbers,
        fc.constantFrom("ms", "s"),
        fc.boolean(),
        async (whole, fraction, unit, colour) => {
          const duration = `${whole}${fraction === undefined ? "" : `.${fraction}`}${unit}`;
          const valid = `built in ${duration}.`;
          const build = createBuildReadiness();
          let isReady = false;
          void build.ready.then(() => {
            isReady = true;
          });
          for (const line of [
            valid.slice(0, -1),
            `error: ${valid}`,
            `built in -${duration}.`,
            `built in ${duration}x.`,
          ]) {
            build.observeLine(line);
          }
          await Promise.resolve();
          expect(isReady).toBe(false);
          build.observeLine(colour ? `\u001B[32m${valid}\u001B[39m` : valid);
          await build.ready;
          expect(isReady).toBe(true);
        }
      ),
      { numRuns: 80 }
    );
  });
});
