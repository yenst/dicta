export type Platform = "macos" | "linux" | "windows" | "unknown";
export type MediaPlaybackStrategy = "direct-asset" | "blob-fallback";

export interface ShortcutOption {
  id: string;
  label: string;
  detail: string;
}

export interface PlatformCapabilities {
  id: Platform;
  displayName: string;
  isMac: boolean;
  isLinux: boolean;
  defaultShortcutId: string;
  revealLabel: string;
  mediaPlayback: MediaPlaybackStrategy;
  qualityModelPath: string;
  mcpExecutablePath: string;
  systemThemeDetail: string;
  shortcutHelp: string;
  shortcutOptions: ShortcutOption[];
  compactPath(path: string): string;
}

export function detectPlatform(platformHint: string): Platform {
  const normalized = platformHint.toLowerCase();
  if (normalized.includes("mac") || normalized.includes("darwin") || normalized.includes("iphone") || normalized.includes("ipad")) return "macos";
  if (normalized.includes("linux")) return "linux";
  if (normalized.includes("win")) return "windows";
  return "unknown";
}

export function createPlatformCapabilities(id: Platform): PlatformCapabilities {
  const isMac = id === "macos";
  const defaultShortcutId = isMac ? "command_shift_r" : "alt_shift_r";
  return {
    id,
    displayName: id === "macos" ? "Mac" : id === "linux" ? "Linux computer" : id === "windows" ? "Windows PC" : "computer",
    isMac,
    isLinux: id === "linux",
    defaultShortcutId,
    revealLabel: isMac ? "Reveal in Finder" : id === "windows" ? "Show in Explorer" : "Show in Files",
    // The current WebKit workaround is needed on every non-macOS build. Keeping
    // that policy here makes a future Windows-specific strategy explicit.
    mediaPlayback: isMac ? "direct-asset" : "blob-fallback",
    qualityModelPath: isMac
      ? "~/Library/Application Support/Dicta/models/ggml-large-v3-turbo-q5_0.bin"
      : id === "windows"
        ? "~/AppData/Local/Dicta/models/ggml-large-v3-turbo-q5_0.bin"
        : "~/.local/share/Dicta/models/ggml-large-v3-turbo-q5_0.bin",
    mcpExecutablePath: isMac
      ? "/Library/Application Support/Dicta/bin/dicta-mcp"
      : id === "windows"
        ? "~/AppData/Local/Dicta/bin/dicta-mcp.exe"
        : "~/.local/share/Dicta/bin/dicta-mcp",
    systemThemeDetail: isMac ? "Follow macOS" : id === "linux" ? "Follow Linux" : id === "windows" ? "Follow Windows" : "Follow system",
    shortcutHelp: isMac
      ? "Double-Fn is reserved by macOS and cannot be registered reliably; these combinations work globally."
      : "Global shortcuts use Super, Alt, or Control and work while Dicta is in the background.",
    shortcutOptions: [
      { id: defaultShortcutId, label: isMac ? "⌘ ⇧ R" : "Alt Shift R", detail: "Default" },
      { id: "command_shift_d", label: isMac ? "⌘ ⇧ D" : "Super Shift D", detail: "Dicta" },
      { id: "option_space", label: isMac ? "⌥ Space" : "Alt Space", detail: "Compact" },
      { id: "control_space", label: isMac ? "⌃ Space" : "Ctrl Space", detail: "Alternate" },
    ],
    compactPath: (path) => compactPathForPlatform(path, id),
  };
}

function compactPathForPlatform(path: string, platform: Platform): string {
  if (platform === "windows") return path.replace(/^[A-Za-z]:\\Users\\[^\\]+/i, "~");
  if (platform === "linux") return path.replace(/^\/home\/[^/]+/, "~");
  if (platform === "macos") return path.replace(/^\/Users\/[^/]+/, "~");
  return path.replace(/^\/(?:Users|home)\/[^/]+/, "~");
}
