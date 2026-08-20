export interface ShellViewModel {
  linux: boolean;
  chrome: string;
  sidebar: string;
  header: string;
  content: string;
  settings: string;
  modals: string;
  toast: string;
}

export function renderShell(vm: ShellViewModel): string {
  return `<main class="app-shell ${vm.linux ? "linux-shell" : ""}">${vm.chrome}${vm.sidebar}<section class="workspace">${vm.header}${vm.content}${vm.settings}</section></main>${vm.modals}${vm.toast}`;
}
