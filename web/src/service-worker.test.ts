// SPDX-License-Identifier: AGPL-3.0-or-later
import { describe, expect, it, vi } from "vitest";
import {
  type ContainerLike,
  type RegistrationLike,
  registerWorker,
  SKIP_WAITING,
  shouldRegister,
  type WorkerLike,
} from "./service-worker";

describe("registering the worker (spec 15 B6)", () => {
  const where = {
    production: true,
    protocol: "https:",
    hostname: "lms.example.org",
    supported: true,
  };

  it("registers in a production build on https or localhost only", () => {
    expect(shouldRegister(where)).toBe(true);
    expect(shouldRegister({ ...where, protocol: "http:", hostname: "localhost" })).toBe(true);
    expect(shouldRegister({ ...where, protocol: "http:", hostname: "127.0.0.1" })).toBe(true);
    expect(shouldRegister({ ...where, production: false })).toBe(false);
    expect(shouldRegister({ ...where, supported: false })).toBe(false);
    expect(shouldRegister({ ...where, protocol: "http:" })).toBe(false);
    expect(shouldRegister({ ...where, protocol: "http:", hostname: "tauri.localhost" })).toBe(
      false,
    );
    expect(shouldRegister({ ...where, protocol: "tauri:", hostname: "localhost" })).toBe(false);
  });
});

class FakeWorker extends EventTarget implements WorkerLike {
  state = "installing";
  messages: unknown[] = [];
  postMessage(message: unknown) {
    this.messages.push(message);
  }
  become(state: string) {
    this.state = state;
    this.dispatchEvent(new Event("statechange"));
  }
}

class FakeRegistration extends EventTarget implements RegistrationLike {
  waiting: FakeWorker | null = null;
  installing: FakeWorker | null = null;
  found(worker: FakeWorker) {
    this.installing = worker;
    this.dispatchEvent(new Event("updatefound"));
  }
}

class FakeContainer extends EventTarget implements ContainerLike {
  controller: unknown = null;
  registration = new FakeRegistration();
  register = vi.fn(async () => this.registration);
  changeController() {
    this.controller = {};
    this.dispatchEvent(new Event("controllerchange"));
  }
}

function setUp(controlled: boolean) {
  const container = new FakeContainer();
  if (controlled) container.controller = {};
  const offers: (() => void)[] = [];
  const reload = vi.fn();
  const stop = new AbortController();
  const hooks = {
    onUpdate: (apply: () => void) => offers.push(apply),
    reload,
    signal: stop.signal,
  };
  return { container, offers, reload, stop, hooks };
}

describe("offering a new build (spec 15 B5)", () => {
  it("registers /sw.js at scope / past the HTTP cache", async () => {
    const { container, hooks } = setUp(false);
    await registerWorker(container, hooks);
    expect(container.register).toHaveBeenCalledWith("/sw.js", {
      scope: "/",
      updateViaCache: "none",
    });
  });

  it("offers nothing for a first install", async () => {
    const { container, offers, hooks } = setUp(false);
    await registerWorker(container, hooks);
    const worker = new FakeWorker();
    container.registration.found(worker);
    worker.become("installed");
    expect(offers).toHaveLength(0);
  });

  it("offers a worker that installs behind the controller once, and Reload activates it", async () => {
    const { container, offers, reload, hooks } = setUp(true);
    await registerWorker(container, hooks);
    const worker = new FakeWorker();
    container.registration.found(worker);
    expect(offers).toHaveLength(0);
    worker.become("installed");
    worker.become("installed");
    expect(offers).toHaveLength(1);
    expect(reload).not.toHaveBeenCalled();
    offers[0]?.();
    expect(worker.messages).toEqual([SKIP_WAITING]);
    container.changeController();
    expect(reload).toHaveBeenCalledOnce();
  });

  it("offers a worker already waiting when the page opens", async () => {
    const { container, offers, hooks } = setUp(true);
    container.registration.waiting = new FakeWorker();
    await registerWorker(container, hooks);
    expect(offers).toHaveLength(1);
  });

  it("never reloads for a change of controller it did not ask for", async () => {
    const { container, reload, hooks } = setUp(true);
    await registerWorker(container, hooks);
    container.changeController();
    expect(reload).not.toHaveBeenCalled();
  });

  it("stops listening once stopped", async () => {
    const { container, offers, stop, hooks } = setUp(true);
    await registerWorker(container, hooks);
    stop.abort();
    const worker = new FakeWorker();
    container.registration.found(worker);
    worker.become("installed");
    expect(offers).toHaveLength(0);
  });
});
