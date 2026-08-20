import { describe, expect, it } from "vitest";
import { RequestGate } from "./request-gate";

function deferred<T>(): { promise: Promise<T>; resolve: (value: T) => void } {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((complete) => { resolve = complete; });
  return { promise, resolve };
}

describe("RequestGate", () => {
  it("rejects a late result from the previously selected project", async () => {
    const gate = new RequestGate();
    const projectA = deferred<string[]>();
    const projectB = deferred<string[]>();
    const committed: string[] = [];

    const load = async (projectId: string, request: Promise<string[]>) => {
      const token = gate.begin(projectId);
      const recordings = await request;
      if (gate.isCurrent(token)) committed.push(...recordings);
    };

    const loadA = load("project-a", projectA.promise);
    const loadB = load("project-b", projectB.promise);
    projectB.resolve(["b-recording"]);
    await loadB;
    projectA.resolve(["a-recording"]);
    await loadA;

    expect(committed).toEqual(["b-recording"]);
  });

  it("invalidates an in-flight request when there is no active project", () => {
    const gate = new RequestGate();
    const token = gate.begin("project-a");
    gate.invalidate();
    expect(gate.isCurrent(token)).toBe(false);
  });
});
