// SPDX-License-Identifier: AGPL-3.0-or-later
import { afterEach, describe, expect, it, vi } from "vitest";
import { isApplePlatform } from "./platform";

function platform(name: string, uaData?: string) {
  vi.stubGlobal("navigator", {
    platform: name,
    userAgentData: uaData === undefined ? undefined : { platform: uaData },
  });
}

describe("isApplePlatform", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("is true on macOS, iPhone and iPad", () => {
    for (const name of ["MacIntel", "iPhone", "iPad"]) {
      platform(name);
      expect(isApplePlatform()).toBe(true);
    }
  });

  it("is false on Windows, Linux and Android", () => {
    for (const name of ["Win32", "Linux x86_64", "Linux armv8l"]) {
      platform(name);
      expect(isApplePlatform()).toBe(false);
    }
  });

  it("prefers the user-agent client hint when the browser has one", () => {
    platform("", "macOS");
    expect(isApplePlatform()).toBe(true);
    platform("MacIntel", "Windows");
    expect(isApplePlatform()).toBe(false);
  });
});
