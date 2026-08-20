export type FocusKey =
  | { kind: "id"; value: string }
  | { kind: "recording"; value: string }
  | { kind: "project"; value: string };

export function captureFocusKey(root: ParentNode, activeElement: Element | null): FocusKey | null {
  if (!(activeElement instanceof HTMLElement) || !root.contains(activeElement)) return null;
  const recordingId = activeElement.dataset.openPacket
    ?? activeElement.dataset.transcribe
    ?? activeElement.dataset.delete
    ?? activeElement.dataset.menu;
  if (recordingId) return { kind: "recording", value: recordingId };
  const projectId = activeElement.dataset.projectId
    ?? activeElement.dataset.projectMenu
    ?? activeElement.dataset.removeProject;
  if (projectId) return { kind: "project", value: projectId };
  if (activeElement.id) return { kind: "id", value: activeElement.id };
  return null;
}

export function restoreFocusKey(root: ParentNode, key: FocusKey | null): boolean {
  if (!key) return false;
  let target: HTMLElement | null = null;
  if (key.kind === "id") target = root.querySelector<HTMLElement>(`#${key.value}`);
  if (key.kind === "recording") {
    target = [...root.querySelectorAll<HTMLElement>("[data-open-packet]")]
      .find((element) => element.dataset.openPacket === key.value) ?? null;
  }
  if (key.kind === "project") {
    target = [...root.querySelectorAll<HTMLElement>("[data-project-id]")]
      .find((element) => element.dataset.projectId === key.value) ?? null;
  }
  if (!target) return false;
  target.focus({ preventScroll: true });
  return true;
}
