const navigationKeys = new Set(["ArrowLeft", "ArrowRight", "Home", "End"]);

export function handleViewerTabKeydown(
  event: KeyboardEvent,
  current: HTMLElement,
  onSelect: (panel: string) => void,
): boolean {
  if (!navigationKeys.has(event.key)) return false;
  const tablist = current.closest<HTMLElement>("[role='tablist']");
  if (!tablist) return false;
  const tabs = [...tablist.querySelectorAll<HTMLElement>("[role='tab'][data-viewer-panel]")];
  const currentIndex = tabs.indexOf(current);
  if (currentIndex < 0 || tabs.length === 0) return false;
  event.preventDefault();
  const nextIndex = event.key === "Home"
    ? 0
    : event.key === "End"
      ? tabs.length - 1
      : event.key === "ArrowRight"
        ? (currentIndex + 1) % tabs.length
        : (currentIndex - 1 + tabs.length) % tabs.length;
  const target = tabs[nextIndex];
  target.focus({ preventScroll: true });
  onSelect(target.dataset.viewerPanel ?? "transcript");
  return true;
}
