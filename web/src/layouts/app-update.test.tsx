// SPDX-License-Identifier: AGPL-3.0-or-later
// The update offer in the app (spec 15 B5): a new build's waiting worker shows "A new version is
// available" with "Reload", which tells the worker to take over and reloads once it does.
import { createToastQueue, StringsProvider, ToastRegion } from "@stepclimb/ui";
import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { catalogs } from "../messages";
import { type ContainerLike, SKIP_WAITING, type WorkerLike } from "../service-worker";
import { AppUpdate } from "./app-update";

class Worker extends EventTarget implements WorkerLike {
  state = "installed";
  postMessage = vi.fn();
}

class Container extends EventTarget implements ContainerLike {
  controller: unknown = {};
  waiting: Worker | null = new Worker();
  register = vi.fn(async () => {
    const registration = new EventTarget() as EventTarget & {
      waiting: Worker | null;
      installing: null;
    };
    registration.waiting = this.waiting;
    registration.installing = null;
    return registration;
  });
}

function show(container: Container, production: boolean, reload = vi.fn()) {
  const toasts = createToastQueue();
  render(
    <StringsProvider catalogs={catalogs} isDevelopment>
      <AppUpdate toasts={toasts} container={container} production={production} reload={reload} />
      <ToastRegion queue={toasts} />
    </StringsProvider>,
  );
  return reload;
}

describe("AppUpdate", () => {
  it("offers a waiting build, and Reload activates it and reloads", async () => {
    const container = new Container();
    const reload = show(container, true);
    const reloadButton = await screen.findByRole("button", { name: "Reload" });
    expect(screen.getByRole("alertdialog", { name: "A new version is available" })).toBeVisible();
    await userEvent.setup().click(reloadButton);
    expect(container.waiting?.postMessage).toHaveBeenCalledWith(SKIP_WAITING);
    expect(reload).not.toHaveBeenCalled();
    act(() => {
      container.dispatchEvent(new Event("controllerchange"));
    });
    expect(reload).toHaveBeenCalledOnce();
  });

  it("registers nothing outside a production build", async () => {
    const container = new Container();
    show(container, false);
    await act(async () => {});
    expect(container.register).not.toHaveBeenCalled();
    expect(screen.queryByRole("alertdialog")).toBeNull();
  });
});
