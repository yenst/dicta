export type DelegatedEventType = "click" | "input" | "change" | "submit" | "keydown";

export interface DelegatedRoute<E extends Event = Event> {
  type: DelegatedEventType;
  selector: string;
  handle(event: E, matched: HTMLElement): void | Promise<void>;
}

export function mountDelegatedEvents(root: HTMLElement, routes: DelegatedRoute[]): () => void {
  const eventTypes = [...new Set(routes.map((route) => route.type))];
  const listeners = eventTypes.map((type) => {
    const listener = (event: Event): void => {
      const target = event.target;
      if (!(target instanceof Element)) return;
      for (const route of routes) {
        if (route.type !== type) continue;
        const matched = target.closest<HTMLElement>(route.selector);
        if (!matched || !root.contains(matched)) continue;
        void route.handle(event, matched);
        return;
      }
    };
    root.addEventListener(type, listener);
    return { type, listener };
  });
  return () => listeners.forEach(({ type, listener }) => root.removeEventListener(type, listener));
}
