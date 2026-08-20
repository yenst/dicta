import { describe, expect, it } from "vitest";
import { createPlatformCapabilities, detectPlatform } from "./platform";

describe("platform capabilities", () => {
  it.each([
    ["MacIntel", "macos"],
    ["Darwin", "macos"],
    ["Linux x86_64", "linux"],
    ["Win32", "windows"],
    ["Plan9", "unknown"],
  ] as const)("detects %s as %s", (hint, expected) => {
    expect(detectPlatform(hint)).toBe(expected);
  });

  it("keeps OS labels, shortcuts, paths, and media policy together", () => {
    const mac = createPlatformCapabilities("macos");
    const linux = createPlatformCapabilities("linux");
    const windows = createPlatformCapabilities("windows");

    expect(mac.defaultShortcutId).toBe("command_shift_r");
    expect(mac.mediaPlayback).toBe("direct-asset");
    expect(mac.compactPath("/Users/jim/Projects/dicta")).toBe("~/Projects/dicta");
    expect(linux.mediaPlayback).toBe("blob-fallback");
    expect(linux.compactPath("/home/jim/Projects/dicta")).toBe("~/Projects/dicta");
    expect(windows.revealLabel).toBe("Show in Explorer");
    expect(windows.compactPath("C:\\Users\\jim\\Projects\\dicta")).toBe("~\\Projects\\dicta");
  });
});
