import { describe, expect, it, vi } from "vitest";
import { AppLifecycle } from "./app-lifecycle";

describe("AppLifecycle", () => {
  it("replaces scoped resources and disposes every resource once", () => {
    const lifecycle = new AppLifecycle();
    const first = vi.fn();
    const second = vi.fn();
    const persistent = vi.fn();
    lifecycle.replace("modal", first);
    lifecycle.replace("modal", second);
    lifecycle.add(persistent);

    expect(first).toHaveBeenCalledOnce();
    lifecycle.dispose();
    lifecycle.dispose();
    expect(second).toHaveBeenCalledOnce();
    expect(persistent).toHaveBeenCalledOnce();
  });

  it("disposes the previous slot before mounting its replacement", () => {
    const lifecycle = new AppLifecycle();
    const order: string[] = [];
    lifecycle.replace("modal", () => { order.push("dispose"); });
    lifecycle.replaceWith("modal", () => {
      order.push("mount");
      return () => undefined;
    });
    expect(order).toEqual(["dispose", "mount"]);
  });
});
