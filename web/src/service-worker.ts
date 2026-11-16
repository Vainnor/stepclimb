// SPDX-License-Identifier: AGPL-3.0-or-later
// Registering the service worker and offering its updates
// (docs/specs/m1/15-installable-web-app-and-offline-reading.md B5, B6). The worker is registered
// only in a production build opened over `https` or on localhost, which leaves out the Vite dev
// server and the Tauri shell (`tauri://localhost`, `http://tauri.localhost`). A new build's worker
// waits; the app offers it, and "Reload" activates it and reloads the page once it controls it.

/** The worker's path and scope. */
export const WORKER_URL = "/sw.js";

/** What the page tells a waiting worker to take over with. */
export const SKIP_WAITING = { type: "skip-waiting" } as const;

const LOCAL_HOSTS = new Set(["localhost", "127.0.0.1", "[::1]"]);

export interface Where {
  production: boolean;
  protocol: string;
  hostname: string;
  /** `"serviceWorker" in navigator`. */
  supported: boolean;
}

/** Whether to register the worker: a production build, on `https` or localhost (B6). */
export function shouldRegister({ production, protocol, hostname, supported }: Where): boolean {
  if (!production || !supported) return false;
  return protocol === "https:" || (protocol === "http:" && LOCAL_HOSTS.has(hostname));
}

/** The parts of a worker the page uses. */
export interface WorkerLike {
  state: string;
  postMessage(message: unknown): void;
  addEventListener(
    type: "statechange",
    listener: () => void,
    options?: AddEventListenerOptions,
  ): void;
}

export interface RegistrationLike {
  waiting: WorkerLike | null;
  installing: WorkerLike | null;
  addEventListener(
    type: "updatefound",
    listener: () => void,
    options?: AddEventListenerOptions,
  ): void;
}

export interface ContainerLike {
  controller: unknown;
  register(url: string, options: RegistrationOptions): Promise<RegistrationLike>;
  addEventListener(
    type: "controllerchange",
    listener: () => void,
    options?: AddEventListenerOptions,
  ): void;
}

export interface UpdateHooks {
  /** Shows the offer; `apply` activates the waiting worker. Called once per waiting worker. */
  onUpdate(apply: () => void): void;
  /** Reloads the page, once the new worker controls it. */
  reload(): void;
  /** Stops listening. */
  signal: AbortSignal;
}

/**
 * Registers the worker and calls `onUpdate` whenever a new build's worker is waiting behind the
 * one that controls this page. A first install, with no controller, offers nothing. The page
 * reloads on a change of controller only after `apply`, so another tab's update never reloads it
 * unasked.
 */
export async function registerWorker(container: ContainerLike, hooks: UpdateHooks): Promise<void> {
  const { signal } = hooks;
  let applying = false;
  const offered = new WeakSet<WorkerLike>();
  const offer = (worker: WorkerLike) => {
    if (signal.aborted || offered.has(worker) || !container.controller) return;
    offered.add(worker);
    hooks.onUpdate(() => {
      applying = true;
      worker.postMessage(SKIP_WAITING);
    });
  };
  container.addEventListener(
    "controllerchange",
    () => {
      if (applying) hooks.reload();
    },
    { signal },
  );
  const registration = await container.register(WORKER_URL, {
    scope: "/",
    updateViaCache: "none",
  });
  if (registration.waiting) offer(registration.waiting);
  registration.addEventListener(
    "updatefound",
    () => {
      const worker = registration.installing;
      if (!worker) return;
      worker.addEventListener(
        "statechange",
        () => {
          if (worker.state === "installed") offer(worker);
        },
        { signal },
      );
    },
    { signal },
  );
}
