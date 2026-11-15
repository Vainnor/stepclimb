// SPDX-License-Identifier: AGPL-3.0-or-later
import { HouseIcon, UserCircleIcon, UsersIcon } from "@phosphor-icons/react";
import { describe, expect, it } from "vitest";
import { type NavRoute, navRoutes, visibleNavEntries } from "./navigation";
import { meFixture, orgId } from "./test/app";

const t = (id: string) => id;

const routes: NavRoute[] = [
  { id: "home", to: "/", icon: HouseIcon, label: () => "Home" },
  { id: "users", to: "/users", icon: UsersIcon, label: () => "Users", permission: "user.manage" },
];

describe("visibleNavEntries (B5, T2)", () => {
  it("leaves out an entry whose permission /me lacks", () => {
    const ids = visibleNavEntries(routes, meFixture(["user.view"]), t).map((entry) => entry.id);
    expect(ids).toEqual(["home"]);
  });

  it("shows an entry whose permission /me holds", () => {
    const ids = visibleNavEntries(routes, meFixture(["user.manage"]), t).map((entry) => entry.id);
    expect(ids).toEqual(["home", "users"]);
  });

  it("shows an entry whose permission is held at an account scope only", () => {
    const me = meFixture();
    me.grants = [
      { scope: { type: "organization", id: orgId }, permissions: ["user.view"] },
      {
        scope: { type: "account", id: "01900000-0000-7000-8000-0000000000aa" },
        permissions: ["user.manage"],
      },
    ];
    expect(visibleNavEntries(routes, me, t).map((entry) => entry.id)).toEqual(["home", "users"]);
  });

  it("labels and links each entry from its route", () => {
    expect(visibleNavEntries(navRoutes, meFixture(), (id) => `[${id}]`)).toEqual([
      { id: "home", href: "/", label: "[nav.home]", icon: HouseIcon },
      { id: "account", href: "/account", label: "[nav.account]", icon: UserCircleIcon },
    ]);
  });
});
