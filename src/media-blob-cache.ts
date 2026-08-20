export interface MediaIdentity {
  projectId: string;
  recordingId: string;
  videoPath: string;
}

interface MediaBlobCacheDependencies {
  fetch: typeof fetch;
  createObjectURL: (blob: Blob) => string;
  revokeObjectURL: (url: string) => void;
}

interface CachedMedia {
  key: string;
  url: string;
}

interface PendingMedia {
  key: string;
  controller: AbortController;
  promise: Promise<string | null>;
}

function browserDependencies(): MediaBlobCacheDependencies {
  return {
    fetch: globalThis.fetch.bind(globalThis),
    createObjectURL: URL.createObjectURL.bind(URL),
    revokeObjectURL: URL.revokeObjectURL.bind(URL),
  };
}

export function mediaIdentityKey(identity: MediaIdentity): string {
  return JSON.stringify([identity.projectId, identity.recordingId, identity.videoPath]);
}

export class MediaBlobCache {
  private cached: CachedMedia | null = null;
  private pending: PendingMedia | null = null;
  private requestedKey: string | null = null;

  constructor(private readonly dependencies: MediaBlobCacheDependencies = browserDependencies()) {}

  get(identity: MediaIdentity): string | null {
    const key = mediaIdentityKey(identity);
    return this.cached?.key === key ? this.cached.url : null;
  }

  async load(identity: MediaIdentity, sourceUrl: string): Promise<string | null> {
    const key = mediaIdentityKey(identity);
    this.requestedKey = key;

    const cachedUrl = this.get(identity);
    if (cachedUrl) return cachedUrl;
    if (this.pending?.key === key) return this.pending.promise;

    this.pending?.controller.abort();
    this.releaseCached();
    const controller = new AbortController();
    const promise = this.fetchBlobUrl(sourceUrl, key, controller);
    const pending = { key, controller, promise };
    this.pending = pending;

    try {
      return await promise;
    } finally {
      if (this.pending === pending) this.pending = null;
    }
  }

  clear(): void {
    this.requestedKey = null;
    this.pending?.controller.abort();
    this.pending = null;
    this.releaseCached();
  }

  dispose(): void {
    this.clear();
  }

  private async fetchBlobUrl(sourceUrl: string, key: string, controller: AbortController): Promise<string | null> {
    try {
      const response = await this.dependencies.fetch(sourceUrl, { signal: controller.signal });
      if (!response.ok) throw new Error(`media request returned ${response.status}`);
      const blob = await response.blob();
      const mediaBlob = blob.type === "video/mp4" ? blob : new Blob([blob], { type: "video/mp4" });
      const url = this.dependencies.createObjectURL(mediaBlob);
      if (controller.signal.aborted || this.requestedKey !== key) {
        this.dependencies.revokeObjectURL(url);
        return null;
      }

      this.releaseCached();
      this.cached = { key, url };
      return url;
    } catch (error) {
      if (controller.signal.aborted || this.requestedKey !== key) return null;
      throw error;
    }
  }

  private releaseCached(): void {
    if (this.cached) this.dependencies.revokeObjectURL(this.cached.url);
    this.cached = null;
  }
}
