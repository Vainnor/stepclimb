// SPDX-License-Identifier: AGPL-3.0-or-later
// Test support: a stand-in for the browser's WebAuthn API, which jsdom lacks. The options the page
// parses come back as they went in, and the ceremonies answer a fixed credential, or throw what a
// test names.
import { vi } from "vitest";

/** What `toJSON()` of the stand-in credential returns. */
export const credentialJson = { id: "Y3JlZA", rawId: "Y3JlZA", type: "public-key", response: {} };

class FakePublicKeyCredential {
  static parseCreationOptionsFromJSON = vi.fn((options: unknown) => ({ parsed: options }));
  static parseRequestOptionsFromJSON = vi.fn((options: unknown) => ({ parsed: options }));
  toJSON() {
    return credentialJson;
  }
}

export interface WebAuthnStub {
  create: ReturnType<typeof vi.fn>;
  get: ReturnType<typeof vi.fn>;
}

/** Installs the stand-in; `create` and `get` resolve to a credential unless a test changes them. */
export function stubWebAuthn(): WebAuthnStub {
  const create = vi.fn(async () => new FakePublicKeyCredential());
  const get = vi.fn(async () => new FakePublicKeyCredential());
  vi.stubGlobal("PublicKeyCredential", FakePublicKeyCredential);
  Object.defineProperty(navigator, "credentials", { value: { create, get }, configurable: true });
  return { create, get };
}

/** Removes the stand-in, as for a browser without WebAuthn. */
export function removeWebAuthn(): void {
  Object.defineProperty(navigator, "credentials", { value: undefined, configurable: true });
}

/** A DOMException named as the browser names a ceremony's failure. */
export function domError(name: string): DOMException {
  return new DOMException(name, name);
}
