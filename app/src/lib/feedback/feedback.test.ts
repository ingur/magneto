import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { prompts } from "./prompts/prompts.svelte";
import { toast, toasts } from "./toasts/toasts.svelte";

describe("toasts store", () => {
  beforeEach(() => {
    toasts.list = [];
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("appends a toast and returns its id", () => {
    const id = toast.info("hello", { duration: 0 });
    expect(toasts.list).toHaveLength(1);
    expect(toasts.list[0].id).toBe(id);
    expect(toasts.list[0].kind).toBe("info");
  });

  it("dismiss(id) removes one; dismiss() clears all", () => {
    toast.info("a", { duration: 0 });
    const id = toast.error("b", { duration: 0 });
    toast.dismiss(id);
    expect(toasts.list).toHaveLength(1);
    expect(toasts.list[0].message).toBe("a");
    toast.success("c", { duration: 0 });
    toast.dismiss();
    expect(toasts.list).toHaveLength(0);
  });

  it("evicts the oldest past the visible cap (5)", () => {
    for (let i = 1; i <= 6; i++) toast.info(`t${i}`, { duration: 0 });
    expect(toasts.list).toHaveLength(5);
    expect(toasts.list[0].message).toBe("t2"); // t1 evicted
    expect(toasts.list[4].message).toBe("t6");
  });

  it("auto-dismisses after the duration", () => {
    vi.useFakeTimers();
    toast.info("x", { duration: 1000 });
    expect(toasts.list).toHaveLength(1);
    vi.advanceTimersByTime(1000);
    expect(toasts.list).toHaveLength(0);
  });

  it("keeps a duration:0 toast until dismissed", () => {
    vi.useFakeTimers();
    toast.info("sticky", { duration: 0 });
    vi.advanceTimersByTime(100_000);
    expect(toasts.list).toHaveLength(1);
  });
});

describe("prompts store", () => {
  beforeEach(() => {
    prompts.queue = [];
  });

  it("enqueues and exposes the head as current", () => {
    const p = prompts.show({ type: "confirm", title: "t" });
    expect(prompts.current?.opts.title).toBe("t");
    prompts.resolve(true);
    return expect(p).resolves.toBe(true);
  });

  it("resolves the awaited promise and advances to the next", async () => {
    const a = prompts.show({ type: "confirm", title: "a" });
    const b = prompts.show({ type: "confirm", title: "b" });
    expect(prompts.current?.opts.title).toBe("a");
    prompts.resolve(true);
    await expect(a).resolves.toBe(true);
    expect(prompts.current?.opts.title).toBe("b"); // FIFO: next takes over
    prompts.resolve(null);
    await expect(b).resolves.toBeNull();
    expect(prompts.current).toBeNull();
  });

  it("carries the committed value through (text)", () => {
    const p = prompts.show({ type: "text", title: "name" });
    prompts.resolve("hello");
    return expect(p).resolves.toBe("hello");
  });

  it("resolve on an empty queue is a no-op", () => {
    expect(() => prompts.resolve(true)).not.toThrow();
  });
});
