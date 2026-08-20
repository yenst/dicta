export interface RequestToken {
  key: string;
  generation: number;
}

export class RequestGate {
  private generation = 0;
  private activeKey: string | null = null;

  begin(key: string): RequestToken {
    this.generation += 1;
    this.activeKey = key;
    return { key, generation: this.generation };
  }

  isCurrent(token: RequestToken): boolean {
    return token.generation === this.generation && token.key === this.activeKey;
  }

  invalidate(): void {
    this.generation += 1;
    this.activeKey = null;
  }
}
