// SPDX-License-Identifier: AGPL-3.0-or-later
// T2 (docs/specs/m0/13-base-ui-components.md B5): every component rendered with a catalog whose
// messages are all «id», and with the caller's content marked the same way, shows no text and no
// accessible name of its own that is not a catalog message.
import { CheckCircleIcon } from "@phosphor-icons/react";
import type { ReactElement } from "react";
import { afterAll, beforeAll, describe, expect, it, vi } from "vitest";
import { Button } from "../primitives/button";
import { Heading } from "../primitives/heading";
import { Kbd } from "../primitives/kbd";
import { Link } from "../primitives/link";
import { Prose } from "../primitives/prose";
import { Separator } from "../primitives/separator";
import { Text } from "../primitives/text";
import { VisuallyHidden } from "../primitives/visually-hidden";
import { renderWithStrings, unmarkedText } from "../test/strings";
import { Icon } from "../theme/icon";
import { ThemeProvider } from "../theme/theme-provider";

const components: [string, ReactElement][] = [
  ["Button", <Button key="b">«caller»</Button>],
  [
    "Heading",
    <Heading key="h" level={2}>
      «caller»
    </Heading>,
  ],
  ["Icon", <Icon key="i" icon={CheckCircleIcon} label="«caller»" />],
  ["Kbd", <Kbd key="k">«caller»</Kbd>],
  [
    "Link (in-app)",
    <Link key="l" href="/courses">
      «caller»
    </Link>,
  ],
  [
    "Link (external)",
    <Link key="x" href="https://other.example.test/">
      «caller»
    </Link>,
  ],
  [
    "Prose",
    <Prose key="p">
      <p>«caller»</p>
    </Prose>,
  ],
  ["Separator", <Separator key="s" />],
  ["Text", <Text key="t">«caller»</Text>],
  [
    "ThemeProvider",
    <ThemeProvider key="tp" theme="light">
      «caller»
    </ThemeProvider>,
  ],
  ["VisuallyHidden", <VisuallyHidden key="v">«caller»</VisuallyHidden>],
];

describe("no hard-coded strings", () => {
  // jsdom has no matchMedia, which ThemeProvider subscribes to.
  beforeAll(() => {
    vi.stubGlobal("matchMedia", () => ({
      matches: false,
      addEventListener: () => undefined,
      removeEventListener: () => undefined,
    }));
  });
  afterAll(() => {
    vi.unstubAllGlobals();
  });

  it.each(components)("%s renders only catalog messages and the caller's content", (_, ui) => {
    const { container } = renderWithStrings(ui, { marked: true });
    expect(unmarkedText(container)).toEqual([]);
  });

  it("notices a hard-coded string", () => {
    const { container } = renderWithStrings(
      <Text>
        «caller»{" "}
        <button type="button" aria-label="Close">
          x
        </button>
      </Text>,
      { marked: true },
    );
    expect(unmarkedText(container)).toEqual(["x", "Close"]);
  });
});
