import { describe, expect, it, vi } from "vitest";
import { MediaBlobCache, mediaIdentityKey, type MediaIdentity } from "./media-blob-cache";

function deferredResponse() {
  let resolve!: (response: Response) => void;
  const promise = new Promise<Response>((complete) => { resolve = complete; });
  return { promise, resolve };
}

function response(contents: string): Response {
  return new Response(new Blob([contents], { type: "video/mp4" }), { status: 200 });
}

const firstIdentity: MediaIdentity = {
  projectId: "project-a",
  recordingId: "shared-id",
  videoPath: "/project-a/shared-id.mp4",
};

describe("MediaBlobCache", () => {
  it("includes project, recording, and path in the cache identity", () => {
    const key = mediaIdentityKey(firstIdentity);
    expect(mediaIdentityKey({ ...firstIdentity, projectId: "project-b" })).not.toBe(key);
    expect(mediaIdentityKey({ ...firstIdentity, recordingId: "other-id" })).not.toBe(key);
    expect(mediaIdentityKey({ ...firstIdentity, videoPath: "/moved/shared-id.mp4" })).not.toBe(key);
  });

  it("does not reuse a recording ID across projects or video paths", async () => {
    const createObjectURL = vi.fn()
      .mockReturnValueOnce("blob:project-a")
      .mockReturnValueOnce("blob:project-b");
    const revokeObjectURL = vi.fn();
    const cache = new MediaBlobCache({
      fetch: vi.fn()
        .mockResolvedValueOnce(response("a"))
        .mockResolvedValueOnce(response("b")),
      createObjectURL,
      revokeObjectURL,
    });

    await expect(cache.load(firstIdentity, "asset://project-a")).resolves.toBe("blob:project-a");
    const secondIdentity = { ...firstIdentity, projectId: "project-b", videoPath: "/project-b/shared-id.mp4" };
    expect(cache.get(secondIdentity)).toBeNull();
    await expect(cache.load(secondIdentity, "asset://project-b")).resolves.toBe("blob:project-b");

    expect(revokeObjectURL).toHaveBeenCalledWith("blob:project-a");
    expect(cache.get(secondIdentity)).toBe("blob:project-b");
  });

  it("revokes a stale result when a newer identity wins the race", async () => {
    const projectA = deferredResponse();
    const projectB = deferredResponse();
    const createObjectURL = vi.fn()
      .mockReturnValueOnce("blob:project-b")
      .mockReturnValueOnce("blob:project-a");
    const revokeObjectURL = vi.fn();
    const fetchMock = vi.fn()
      .mockImplementationOnce(() => projectA.promise)
      .mockImplementationOnce(() => projectB.promise);
    const cache = new MediaBlobCache({ fetch: fetchMock, createObjectURL, revokeObjectURL });

    const loadA = cache.load(firstIdentity, "asset://project-a");
    const secondIdentity = { ...firstIdentity, projectId: "project-b", videoPath: "/project-b/shared-id.mp4" };
    const loadB = cache.load(secondIdentity, "asset://project-b");
    projectB.resolve(response("b"));
    await expect(loadB).resolves.toBe("blob:project-b");
    projectA.resolve(response("a"));
    await expect(loadA).resolves.toBeNull();

    expect(revokeObjectURL).toHaveBeenCalledWith("blob:project-a");
    expect(cache.get(secondIdentity)).toBe("blob:project-b");
  });

  it("aborts pending work and revokes the cached URL on disposal", async () => {
    const revokeObjectURL = vi.fn();
    const cache = new MediaBlobCache({
      fetch: vi.fn().mockResolvedValue(response("a")),
      createObjectURL: vi.fn().mockReturnValue("blob:project-a"),
      revokeObjectURL,
    });

    await cache.load(firstIdentity, "asset://project-a");
    cache.dispose();

    expect(cache.get(firstIdentity)).toBeNull();
    expect(revokeObjectURL).toHaveBeenCalledWith("blob:project-a");
  });
});
