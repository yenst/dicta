export type Disposer = () => void;

export class AppLifecycle {
  private readonly disposers = new Set<Disposer>();
  private readonly slots = new Map<string, Disposer>();
  private disposed = false;

  add(disposer: Disposer): Disposer {
    if (this.disposed) {
      disposer();
      return disposer;
    }
    this.disposers.add(disposer);
    return disposer;
  }

  replace(slot: string, disposer: Disposer): void {
    this.slots.get(slot)?.();
    if (this.disposed) {
      disposer();
      return;
    }
    this.slots.set(slot, disposer);
  }

  replaceWith(slot: string, mount: () => Disposer): void {
    this.slots.get(slot)?.();
    this.slots.delete(slot);
    const disposer = mount();
    if (this.disposed) {
      disposer();
      return;
    }
    this.slots.set(slot, disposer);
  }

  listen<K extends keyof WindowEventMap>(target: Window, type: K, listener: (event: WindowEventMap[K]) => void, options?: AddEventListenerOptions): void;
  listen<K extends keyof MediaQueryListEventMap>(target: MediaQueryList, type: K, listener: (event: MediaQueryListEventMap[K]) => void, options?: AddEventListenerOptions): void;
  listen(target: Window | MediaQueryList, type: string, listener: EventListener, options?: AddEventListenerOptions): void {
    target.addEventListener(type, listener, options);
    this.add(() => target.removeEventListener(type, listener, options));
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    for (const disposer of this.slots.values()) disposer();
    this.slots.clear();
    for (const disposer of this.disposers) disposer();
    this.disposers.clear();
  }
}
