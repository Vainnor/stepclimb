// SPDX-License-Identifier: AGPL-3.0-or-later
// The account page in jsdom (docs/specs/m0/04-local-accounts.md B13; 11-app-shell.md Q2, B17):
// the passkey list, adding, renaming and deleting against a stubbed server and WebAuthn, the ask
// before a delete, and the message for each refusal the passkey routes return.
import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MyPasskey } from "../account/passkeys";
import {
  type Handler,
  json,
  meFixture,
  problem,
  renderApp,
  stubMatchMedia,
  stubServer,
} from "../test/app";
import { credentialJson, domError, removeWebAuthn, stubWebAuthn } from "../test/webauthn";

const laptop: MyPasskey = {
  id: "01900000-0000-7000-8000-0000000000a1",
  name: "Work laptop",
  created_at: "2026-11-02T12:00:00Z",
  last_used_at: "2026-11-10T12:00:00Z",
};
const phone: MyPasskey = {
  id: "01900000-0000-7000-8000-0000000000a2",
  name: "Phone",
  created_at: "2026-11-05T12:00:00Z",
  last_used_at: null,
};
const creationOptions = {
  publicKey: { challenge: "Y2g", rp: { id: "localhost", name: "Example" } },
};

beforeEach(() => stubMatchMedia(true));
afterEach(() => {
  vi.unstubAllGlobals();
  removeWebAuthn();
});

/** The server of a signed-in user with `passkeys`, plus `more` routes. */
function signedIn(passkeys: MyPasskey[], more: Record<string, Handler> = {}) {
  return stubServer({
    "GET /api/v1/me": () => json(meFixture()),
    "GET /api/v1/me/passkeys": () => json(passkeys),
    ...more,
  });
}

function requestsTo(fetch: ReturnType<typeof stubServer>, method: string, path: string) {
  return fetch.mock.calls
    .map(([r]) => r as Request)
    .filter((r) => r.method === method && new URL(r.url).pathname === path);
}

const list = () => screen.findByRole("grid", { name: "Passkeys" });

async function openAdd() {
  const user = userEvent.setup();
  await user.click(await screen.findByRole("button", { name: "Add a passkey" }));
  const dialog = await screen.findByRole("dialog", { name: "Add a passkey" });
  return { user, dialog };
}

async function create(user: ReturnType<typeof userEvent.setup>, dialog: HTMLElement) {
  const button = within(dialog).getByRole("button", { name: "Create the passkey" });
  await waitFor(() => expect(button).not.toHaveAttribute("data-pending"));
  await user.click(button);
}

describe("the account page", () => {
  it("is a navigation entry and lists each passkey's name, added and last used dates", async () => {
    stubWebAuthn();
    signedIn([laptop, phone]);
    renderApp("/account");
    expect(await screen.findByRole("heading", { level: 1, name: "Account" })).toBeInTheDocument();
    const nav = document.querySelector<HTMLElement>('[data-slot="side-nav"]');
    if (nav === null) throw new Error("no SideNav");
    const entry = within(nav).getByRole("link", { name: "Account" });
    expect(entry).toHaveAttribute("href", "/account");
    expect(entry).toHaveAttribute("aria-current", "page");
    const rows = within(await list()).getAllByRole("row");
    expect(rows).toHaveLength(2);
    expect(rows[0]).toHaveTextContent("Work laptop");
    expect(rows[0]).toHaveTextContent("Added Nov 2, 2026 · Last used Nov 10, 2026");
    expect(rows[1]).toHaveTextContent("Phone");
    expect(rows[1]).toHaveTextContent("Added Nov 5, 2026 · Not used to sign in yet");
    await waitFor(() => expect(document.title).toBe("Account · Example Training"));
  });

  it("says when there are no passkeys", async () => {
    stubWebAuthn();
    signedIn([]);
    renderApp("/account");
    expect(await screen.findByText("You have no passkeys yet.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Add a passkey" })).toBeInTheDocument();
  });

  it("says passkeys are off when the server answers auth.method_disabled, and offers no add", async () => {
    stubWebAuthn();
    stubServer({
      "GET /api/v1/me": () => json(meFixture()),
      "GET /api/v1/me/passkeys": () => problem(404, "auth.method_disabled", "Not Found"),
    });
    renderApp("/account");
    expect(
      await screen.findByText(/Your organization has turned off passkeys/),
    ).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Add a passkey" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Try again" })).not.toBeInTheDocument();
  });

  it("offers Try again when the list fails otherwise", async () => {
    stubWebAuthn();
    let fail = true;
    signedIn([], {
      "GET /api/v1/me/passkeys": () =>
        fail ? problem(500, "internal", "Internal Server Error") : json([laptop]),
    });
    renderApp("/account");
    const retry = await screen.findByRole("button", { name: "Try again" });
    fail = false;
    await userEvent.setup().click(retry);
    expect(within(await list()).getAllByRole("row")).toHaveLength(1);
  });

  it("says why it cannot add a passkey in a browser without WebAuthn", async () => {
    removeWebAuthn();
    vi.stubGlobal("isSecureContext", true);
    signedIn([laptop]);
    renderApp("/account");
    expect(await screen.findByText(/This browser cannot use passkeys/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Add a passkey" })).not.toBeInTheDocument();
  });
});

describe("adding a passkey", () => {
  it("starts when the dialog opens, then sends the credential with the name", async () => {
    const webauthn = stubWebAuthn();
    const added: MyPasskey = { ...phone, name: "Tablet" };
    const fetch = signedIn([laptop], {
      "POST /api/v1/me/passkeys/start": () => json({ options: creationOptions }),
      "POST /api/v1/me/passkeys": () => json(added, 201),
    });
    renderApp("/account");
    const { user, dialog } = await openAdd();
    await waitFor(() =>
      expect(requestsTo(fetch, "POST", "/api/v1/me/passkeys/start")).toHaveLength(1),
    );
    await user.type(within(dialog).getByRole("textbox", { name: "Name" }), "Tablet");
    await create(user, dialog);

    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(webauthn.create).toHaveBeenCalledTimes(1);
    const [request] = requestsTo(fetch, "POST", "/api/v1/me/passkeys");
    expect(await request?.clone().json()).toEqual({ credential: credentialJson, name: "Tablet" });
    const rows = within(await list()).getAllByRole("row");
    expect(rows.map((row) => row.textContent)).toEqual([
      expect.stringContaining("Work laptop"),
      expect.stringContaining("Tablet"),
    ]);
    expect(await screen.findByText("Tablet added")).toBeInTheDocument();
  });

  it("leaves a blank name out, so the server names the passkey", async () => {
    stubWebAuthn();
    const fetch = signedIn([], {
      "POST /api/v1/me/passkeys/start": () => json({ options: creationOptions }),
      "POST /api/v1/me/passkeys": () => json({ ...phone, name: "Passkey" }, 201),
    });
    renderApp("/account");
    const { user, dialog } = await openAdd();
    await user.type(within(dialog).getByRole("textbox", { name: "Name" }), "   ");
    await create(user, dialog);
    await waitFor(() => expect(requestsTo(fetch, "POST", "/api/v1/me/passkeys")).toHaveLength(1));
    const [request] = requestsTo(fetch, "POST", "/api/v1/me/passkeys");
    expect(await request?.clone().json()).toEqual({ credential: credentialJson });
  });

  it.each([
    ["local.passkeys_not_allowed", 403, /sign-in settings for this provider do not allow passkeys/],
    ["auth.method_disabled", 404, /Your organization has turned off passkeys/],
  ])("explains %s from the start route and offers only Cancel", async (code, status, message) => {
    const webauthn = stubWebAuthn();
    signedIn([laptop], {
      "POST /api/v1/me/passkeys/start": () => problem(status, code, "Refused"),
    });
    renderApp("/account");
    const { dialog } = await openAdd();
    expect(await within(dialog).findByRole("alert")).toHaveTextContent(message);
    expect(within(dialog).queryByRole("button", { name: "Create the passkey" })).toBeNull();
    expect(within(dialog).getByRole("button", { name: "Cancel" })).toBeInTheDocument();
    expect(webauthn.create).not.toHaveBeenCalled();
  });

  it.each([
    ["auth.flow_expired", "The passkey registration expired. Try again."],
    ["auth.flow_invalid", "This passkey registration was already finished or replaced"],
    ["auth.passkey_invalid", "Stepclimb could not add that passkey."],
  ])("explains %s and asks for new options", async (code, message) => {
    stubWebAuthn();
    const fetch = signedIn([], {
      "POST /api/v1/me/passkeys/start": () => json({ options: creationOptions }),
      "POST /api/v1/me/passkeys": () => problem(400, code, "Bad Request"),
    });
    renderApp("/account");
    const { user, dialog } = await openAdd();
    await create(user, dialog);
    expect(await within(dialog).findByRole("alert")).toHaveTextContent(message);
    await waitFor(() =>
      expect(requestsTo(fetch, "POST", "/api/v1/me/passkeys/start")).toHaveLength(2),
    );
  });

  it("shows a name the server refuses at the field", async () => {
    stubWebAuthn();
    signedIn([], {
      "POST /api/v1/me/passkeys/start": () => json({ options: creationOptions }),
      "POST /api/v1/me/passkeys": () =>
        new Response(
          JSON.stringify({
            type: "about:blank",
            title: "Unprocessable Entity",
            status: 422,
            detail: "invalid",
            code: "request.invalid",
            errors: [{ field: "/name", code: "name.too_long" }],
          }),
          { status: 422, headers: { "content-type": "application/problem+json" } },
        ),
    });
    renderApp("/account");
    const { user, dialog } = await openAdd();
    await user.type(within(dialog).getByRole("textbox", { name: "Name" }), "x".repeat(201));
    await create(user, dialog);
    const field = within(dialog).getByRole("textbox", { name: "Name" });
    await waitFor(() => expect(field).toHaveAttribute("aria-invalid", "true"));
    expect(within(dialog).getByText("Use at most 200 characters.")).toBeInTheDocument();
    expect(within(dialog).queryByRole("alert")).toBeNull();
  });

  it("says when this device already holds a passkey for the account", async () => {
    const webauthn = stubWebAuthn();
    webauthn.create.mockRejectedValue(domError("InvalidStateError"));
    const fetch = signedIn([laptop], {
      "POST /api/v1/me/passkeys/start": () => json({ options: creationOptions }),
    });
    renderApp("/account");
    const { user, dialog } = await openAdd();
    await create(user, dialog);
    expect(await within(dialog).findByRole("alert")).toHaveTextContent(
      "This device already holds a passkey for your account.",
    );
    expect(requestsTo(fetch, "POST", "/api/v1/me/passkeys")).toHaveLength(0);
  });
});

describe("renaming a passkey", () => {
  it("sends the new name and shows it", async () => {
    stubWebAuthn();
    const fetch = signedIn([laptop], {
      [`PATCH /api/v1/me/passkeys/${laptop.id}`]: () => json({ ...laptop, name: "Home desktop" }),
    });
    renderApp("/account");
    const user = userEvent.setup();
    await user.click(await screen.findByRole("button", { name: "Rename Work laptop" }));
    const dialog = await screen.findByRole("dialog", { name: "Rename passkey" });
    const field = within(dialog).getByRole("textbox", { name: "Name" });
    expect(field).toHaveValue("Work laptop");
    await user.clear(field);
    await user.type(field, "Home desktop");
    await user.click(within(dialog).getByRole("button", { name: "Save name" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    const [request] = requestsTo(fetch, "PATCH", `/api/v1/me/passkeys/${laptop.id}`);
    expect(await request?.clone().json()).toEqual({ name: "Home desktop" });
    expect(within(await list()).getByRole("row")).toHaveTextContent("Home desktop");
  });

  it("shows an empty name the server refuses at the field", async () => {
    stubWebAuthn();
    signedIn([laptop], {
      [`PATCH /api/v1/me/passkeys/${laptop.id}`]: () =>
        new Response(
          JSON.stringify({
            type: "about:blank",
            title: "Unprocessable Entity",
            status: 422,
            detail: "invalid",
            code: "request.invalid",
            errors: [{ field: "/name", code: "name.empty" }],
          }),
          { status: 422, headers: { "content-type": "application/problem+json" } },
        ),
    });
    renderApp("/account");
    const user = userEvent.setup();
    await user.click(await screen.findByRole("button", { name: "Rename Work laptop" }));
    const dialog = await screen.findByRole("dialog", { name: "Rename passkey" });
    await user.clear(within(dialog).getByRole("textbox", { name: "Name" }));
    await user.click(within(dialog).getByRole("button", { name: "Save name" }));
    expect(await within(dialog).findByText("Enter a name.")).toBeInTheDocument();
  });

  it("says a passkey deleted elsewhere is gone, and refreshes the list", async () => {
    stubWebAuthn();
    let passkeys = [laptop];
    const fetch = stubServer({
      "GET /api/v1/me": () => json(meFixture()),
      "GET /api/v1/me/passkeys": () => json(passkeys),
      [`PATCH /api/v1/me/passkeys/${laptop.id}`]: () => {
        passkeys = [];
        return problem(404, "passkey.not_found", "Not Found");
      },
    });
    renderApp("/account");
    const user = userEvent.setup();
    await user.click(await screen.findByRole("button", { name: "Rename Work laptop" }));
    const dialog = await screen.findByRole("dialog", { name: "Rename passkey" });
    await user.click(within(dialog).getByRole("button", { name: "Save name" }));
    expect(await within(dialog).findByRole("alert")).toHaveTextContent(
      "This passkey no longer exists.",
    );
    await waitFor(() => expect(requestsTo(fetch, "GET", "/api/v1/me/passkeys")).toHaveLength(2));
  });
});

describe("deleting a passkey", () => {
  const deleteRoute = (handler: Handler) => ({
    [`DELETE /api/v1/me/passkeys/${laptop.id}`]: handler,
  });

  it("asks first, and Cancel sends nothing", async () => {
    stubWebAuthn();
    const fetch = signedIn(
      [laptop, phone],
      deleteRoute(() => new Response(null, { status: 204 })),
    );
    renderApp("/account");
    const user = userEvent.setup();
    await user.click(await screen.findByRole("button", { name: "Delete Work laptop" }));
    const dialog = await screen.findByRole("alertdialog", { name: "Delete Work laptop?" });
    expect(dialog).toHaveTextContent("You can no longer sign in with this passkey.");
    expect(requestsTo(fetch, "DELETE", `/api/v1/me/passkeys/${laptop.id}`)).toHaveLength(0);
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument());
    expect(requestsTo(fetch, "DELETE", `/api/v1/me/passkeys/${laptop.id}`)).toHaveLength(0);
    expect(within(await list()).getAllByRole("row")).toHaveLength(2);
  });

  it("deletes once confirmed, drops it from the list and says so", async () => {
    stubWebAuthn();
    const fetch = signedIn(
      [laptop, phone],
      deleteRoute(() => new Response(null, { status: 204 })),
    );
    renderApp("/account");
    const user = userEvent.setup();
    await user.click(await screen.findByRole("button", { name: "Delete Work laptop" }));
    const dialog = await screen.findByRole("alertdialog", { name: "Delete Work laptop?" });
    await user.click(within(dialog).getByRole("button", { name: "Delete passkey" }));
    expect(await screen.findByText("Work laptop deleted")).toBeInTheDocument();
    expect(requestsTo(fetch, "DELETE", `/api/v1/me/passkeys/${laptop.id}`)).toHaveLength(1);
    const rows = within(await list()).getAllByRole("row");
    expect(rows).toHaveLength(1);
    expect(rows[0]).toHaveTextContent("Phone");
    expect(screen.getByRole("heading", { level: 2, name: "Passkeys" })).toHaveFocus();
  });

  it("explains local.last_method and keeps the passkey", async () => {
    stubWebAuthn();
    signedIn(
      [laptop],
      deleteRoute(() => problem(409, "local.last_method", "Conflict")),
    );
    renderApp("/account");
    const user = userEvent.setup();
    await user.click(await screen.findByRole("button", { name: "Delete Work laptop" }));
    const dialog = await screen.findByRole("alertdialog", { name: "Delete Work laptop?" });
    await user.click(within(dialog).getByRole("button", { name: "Delete passkey" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "This passkey is your last way to sign in, so it cannot be deleted. Add another passkey first",
    );
    expect(within(await list()).getAllByRole("row")).toHaveLength(1);
  });

  it("shows an unknown code beside the general message", async () => {
    stubWebAuthn();
    signedIn(
      [laptop],
      deleteRoute(() => problem(403, "auth.csrf_refused", "Forbidden")),
    );
    renderApp("/account");
    const user = userEvent.setup();
    await user.click(await screen.findByRole("button", { name: "Delete Work laptop" }));
    const dialog = await screen.findByRole("alertdialog", { name: "Delete Work laptop?" });
    await user.click(within(dialog).getByRole("button", { name: "Delete passkey" }));
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Something went wrong. Try again.");
    expect(alert).toHaveTextContent("auth.csrf_refused");
  });
});

/** A response the test releases when it chooses. */
function held() {
  let release: (response: Response) => void = () => undefined;
  const answer = new Promise<Response>((resolve) => {
    release = resolve;
  });
  return { answer, release: (response: Response) => release(response) };
}

describe("the account page's states", () => {
  it("shows a skeleton, not the list, while the list loads", async () => {
    stubWebAuthn();
    const list = held();
    signedIn([], { "GET /api/v1/me/passkeys": () => list.answer });
    renderApp("/account");
    await screen.findByRole("heading", { level: 1, name: "Account" });
    await waitFor(() =>
      expect(document.querySelector('[aria-busy="true"] [data-slot="skeleton"]')).not.toBeNull(),
    );
    expect(screen.queryByRole("grid")).toBeNull();
    expect(screen.queryByRole("button", { name: "Add a passkey" })).toBeNull();
    list.release(json([laptop]));
    expect(
      within(await screen.findByRole("grid", { name: "Passkeys" })).getAllByRole("row"),
    ).toHaveLength(1);
    expect(document.querySelector('[aria-busy="true"]')).toBeNull();
  });

  it("sends a 401 from the list to the sign-in screen with /account as return target", async () => {
    stubWebAuthn();
    signedIn([], {
      "GET /api/v1/me/passkeys": () => problem(401, "auth.unauthenticated", "Unauthorized"),
    });
    const app = renderApp("/account");
    expect(await screen.findByRole("heading", { level: 1, name: "Sign in" })).toBeInTheDocument();
    expect(app.router.state.location.search).toEqual({ return_to: "/account" });
  });

  it("sends a 401 from a delete to the sign-in screen", async () => {
    stubWebAuthn();
    signedIn([laptop], {
      [`DELETE /api/v1/me/passkeys/${laptop.id}`]: () =>
        problem(401, "auth.unauthenticated", "Unauthorized"),
    });
    const app = renderApp("/account");
    const user = userEvent.setup();
    await user.click(await screen.findByRole("button", { name: "Delete Work laptop" }));
    const dialog = await screen.findByRole("alertdialog", { name: "Delete Work laptop?" });
    await user.click(within(dialog).getByRole("button", { name: "Delete passkey" }));
    expect(await screen.findByRole("heading", { level: 1, name: "Sign in" })).toBeInTheDocument();
    expect(app.router.state.location.search).toEqual({ return_to: "/account" });
  });
});

describe("one request at a time", () => {
  it("ignores Enter in the add dialog while the passkey is being added", async () => {
    const webauthn = stubWebAuthn();
    const added = held();
    const fetch = signedIn([], {
      "POST /api/v1/me/passkeys/start": () => json({ options: creationOptions }),
      "POST /api/v1/me/passkeys": () => added.answer,
    });
    renderApp("/account");
    const { user, dialog } = await openAdd();
    const field = within(dialog).getByRole("textbox", { name: "Name" });
    await user.type(field, "Tablet");
    await create(user, dialog);
    await waitFor(() => expect(requestsTo(fetch, "POST", "/api/v1/me/passkeys")).toHaveLength(1));
    await user.type(field, "{Enter}");
    await user.type(field, "{Enter}");
    expect(webauthn.create).toHaveBeenCalledTimes(1);
    expect(requestsTo(fetch, "POST", "/api/v1/me/passkeys")).toHaveLength(1);
    added.release(json({ ...phone, name: "Tablet" }, 201));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  });

  it("ignores Enter in the rename dialog while the rename is in flight", async () => {
    stubWebAuthn();
    const renamed = held();
    const path = `/api/v1/me/passkeys/${laptop.id}`;
    const fetch = signedIn([laptop], { [`PATCH ${path}`]: () => renamed.answer });
    renderApp("/account");
    const user = userEvent.setup();
    await user.click(await screen.findByRole("button", { name: "Rename Work laptop" }));
    const dialog = await screen.findByRole("dialog", { name: "Rename passkey" });
    const field = within(dialog).getByRole("textbox", { name: "Name" });
    await user.type(field, "{Enter}");
    await waitFor(() => expect(requestsTo(fetch, "PATCH", path)).toHaveLength(1));
    await user.type(field, "{Enter}");
    await user.type(field, "{Enter}");
    expect(requestsTo(fetch, "PATCH", path)).toHaveLength(1);
    renamed.release(json(laptop));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  });

  it("labels the add button Try again after the start failed, and starts again", async () => {
    stubWebAuthn();
    let fail = true;
    const fetch = signedIn([], {
      "POST /api/v1/me/passkeys/start": () =>
        fail
          ? problem(500, "internal", "Internal Server Error")
          : json({ options: creationOptions }),
    });
    renderApp("/account");
    const { user, dialog } = await openAdd();
    const retry = await within(dialog).findByRole("button", { name: "Try again" });
    fail = false;
    await user.click(retry);
    expect(
      await within(dialog).findByRole("button", { name: "Create the passkey" }),
    ).toBeInTheDocument();
    expect(requestsTo(fetch, "POST", "/api/v1/me/passkeys/start")).toHaveLength(2);
  });
});

describe("the last-method refusal", () => {
  async function refuseLast(user: ReturnType<typeof userEvent.setup>) {
    await user.click(await screen.findByRole("button", { name: "Delete Work laptop" }));
    const dialog = await screen.findByRole("alertdialog", { name: "Delete Work laptop?" });
    await user.click(within(dialog).getByRole("button", { name: "Delete passkey" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("your last way to sign in");
  }
  const refusing = {
    [`DELETE /api/v1/me/passkeys/${laptop.id}`]: () =>
      problem(409, "local.last_method", "Conflict"),
  };

  it("goes once a passkey is added", async () => {
    stubWebAuthn();
    signedIn([laptop], {
      ...refusing,
      "POST /api/v1/me/passkeys/start": () => json({ options: creationOptions }),
      "POST /api/v1/me/passkeys": () => json(phone, 201),
    });
    renderApp("/account");
    const user = userEvent.setup();
    await refuseLast(user);
    const dialog = await openAdd().then((opened) => opened.dialog);
    await create(user, dialog);
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(screen.queryByText(/your last way to sign in/)).toBeNull();
  });

  it("goes once a passkey is renamed", async () => {
    stubWebAuthn();
    signedIn([laptop], {
      ...refusing,
      [`PATCH /api/v1/me/passkeys/${laptop.id}`]: () => json({ ...laptop, name: "Desk" }),
    });
    renderApp("/account");
    const user = userEvent.setup();
    await refuseLast(user);
    await user.click(screen.getByRole("button", { name: "Rename Work laptop" }));
    const dialog = await screen.findByRole("dialog", { name: "Rename passkey" });
    await user.click(within(dialog).getByRole("button", { name: "Save name" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(screen.queryByText(/your last way to sign in/)).toBeNull();
  });
});
