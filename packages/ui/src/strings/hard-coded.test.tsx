// SPDX-License-Identifier: AGPL-3.0-or-later
// T2 (docs/specs/m0/13-base-ui-components.md B5): every component rendered with a catalog whose
// messages are all «id», and with the caller's content marked the same way, shows no text and no
// accessible name of its own that is not a catalog message.
import { CheckCircleIcon, GearIcon } from "@phosphor-icons/react";
import type { ReactElement } from "react";
import { afterAll, beforeAll, describe, expect, it, vi } from "vitest";
import { EmptyState } from "../patterns/empty-state";
import { ErrorState } from "../patterns/error-state";
import { Avatar } from "../primitives/avatar";
import { Button } from "../primitives/button";
import { ButtonGroup } from "../primitives/button-group";
import { CopyButton } from "../primitives/copy-button";
import { Heading } from "../primitives/heading";
import { IconButton } from "../primitives/icon-button";
import { Kbd } from "../primitives/kbd";
import { Link } from "../primitives/link";
import { Menu, MenuItem, MenuTrigger } from "../primitives/menu";
import { Meter } from "../primitives/meter";
import { NumberField } from "../primitives/number-field";
import { Popover, PopoverDialog, PopoverTrigger } from "../primitives/popover";
import { ProgressBar } from "../primitives/progress-bar";
import { Prose } from "../primitives/prose";
import { SearchField } from "../primitives/search-field";
import { Separator } from "../primitives/separator";
import { Text } from "../primitives/text";
import { TextArea } from "../primitives/text-area";
import { TextField } from "../primitives/text-field";
import { ToggleButton, ToggleButtonGroup } from "../primitives/toggle-button";
import { Toolbar } from "../primitives/toolbar";
import { Tooltip, TooltipTrigger } from "../primitives/tooltip";
import { VisuallyHidden } from "../primitives/visually-hidden";
import { renderWithStrings, unmarkedText } from "../test/strings";
import { Icon } from "../theme/icon";
import { ThemeProvider } from "../theme/theme-provider";

const components: [string, ReactElement][] = [
  ["Avatar", <Avatar key="av" name="«caller»" />],
  ["Button", <Button key="b">«caller»</Button>],
  [
    "Button (pending)",
    <Button key="bp" isPending>
      «caller»
    </Button>,
  ],
  [
    "ButtonGroup",
    <ButtonGroup key="bg" aria-label="«caller»">
      <Button>«caller»</Button>
    </ButtonGroup>,
  ],
  ["CopyButton", <CopyButton key="cb" value="«caller»" />],
  [
    "EmptyState",
    <EmptyState key="es" icon={CheckCircleIcon} action={<Button>«caller»</Button>}>
      «caller»
    </EmptyState>,
  ],
  [
    "ErrorState",
    <ErrorState key="er" level={2} title="«caller»" action={<Button>«caller»</Button>}>
      «caller»
    </ErrorState>,
  ],
  [
    "Heading",
    <Heading key="h" level={2}>
      «caller»
    </Heading>,
  ],
  ["Icon", <Icon key="i" icon={CheckCircleIcon} label="«caller»" />],
  ["IconButton", <IconButton key="ib" icon={GearIcon} label="«caller»" />],
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
    "Menu (open)",
    <MenuTrigger key="m" defaultOpen>
      <Button>«caller»</Button>
      <Menu>
        <MenuItem>«caller»</MenuItem>
      </Menu>
    </MenuTrigger>,
  ],
  ["Meter", <Meter key="me" label="«caller»" value={40} />],
  ["NumberField", <NumberField key="nf" label="«caller»" description="«caller»" />],
  [
    "Popover (open)",
    <PopoverTrigger key="po" defaultOpen>
      <Button>«caller»</Button>
      <Popover>
        <PopoverDialog aria-label="«caller»">«caller»</PopoverDialog>
      </Popover>
    </PopoverTrigger>,
  ],
  ["ProgressBar", <ProgressBar key="pb" label="«caller»" value={40} />],
  [
    "Prose",
    <Prose key="p">
      <p>«caller»</p>
    </Prose>,
  ],
  ["SearchField", <SearchField key="sf" label="«caller»" defaultValue="«caller»" />],
  ["Separator", <Separator key="s" />],
  ["Text", <Text key="t">«caller»</Text>],
  ["TextArea", <TextArea key="ta" label="«caller»" isInvalid errorMessage="«caller»" />],
  ["TextField", <TextField key="tf" label="«caller»" isInvalid errorMessage="«caller»" />],
  [
    "ThemeProvider",
    <ThemeProvider key="tp" theme="light">
      «caller»
    </ThemeProvider>,
  ],
  [
    "ToggleButtonGroup",
    <ToggleButtonGroup key="tg" aria-label="«caller»">
      <ToggleButton id="a">«caller»</ToggleButton>
    </ToggleButtonGroup>,
  ],
  [
    "Toolbar",
    <Toolbar key="tb" aria-label="«caller»">
      <Button>«caller»</Button>
    </Toolbar>,
  ],
  [
    "Tooltip (open)",
    <TooltipTrigger key="tt" defaultOpen>
      <Button>«caller»</Button>
      <Tooltip>«caller»</Tooltip>
    </TooltipTrigger>,
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

  // React Aria's modal overlays put a hidden "Dismiss" button at each end for screen readers,
  // labeled from React Aria's own translations, with no prop to pass a label. It is the one text
  // a component shows that is not a Stepclimb message.
  const reactAriaDismiss = (text: string) => text !== "Dismiss";

  it.each(components)("%s renders only catalog messages and the caller's content", (_, ui) => {
    renderWithStrings(ui, { marked: true });
    // The whole body: overlays render outside the container.
    expect(unmarkedText(document.body).filter(reactAriaDismiss)).toEqual([]);
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
