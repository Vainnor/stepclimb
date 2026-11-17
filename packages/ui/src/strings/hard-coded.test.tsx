// SPDX-License-Identifier: AGPL-3.0-or-later
// T2 (docs/specs/m0/13-base-ui-components.md B5): every component rendered with a catalog whose
// messages are all «id», and with the caller's content marked the same way, shows no text and no
// accessible name of its own that is not a catalog message.
import { parseZonedDateTime } from "@internationalized/date";
import { CheckCircleIcon, GearIcon, HouseIcon } from "@phosphor-icons/react";
import type { ReactElement } from "react";
import { afterAll, beforeAll, describe, expect, it, vi } from "vitest";
import { Announcement } from "../learning/announcement";
import { Callout } from "../learning/callout";
import { DueBadge } from "../learning/due-badge";
import { NextUp } from "../learning/next-up";
import { ProgressRail } from "../learning/progress-rail";
import { RequirementLabel } from "../learning/requirement-label";
import { StatusChip } from "../learning/status-chip";
import { AppShell } from "../patterns/app-shell";
import { BottomTabs } from "../patterns/bottom-tabs";
import { CommandPalette } from "../patterns/command-palette";
import { DataTable } from "../patterns/data-table";
import { EmptyState } from "../patterns/empty-state";
import { ErrorState } from "../patterns/error-state";
import { FileDrop } from "../patterns/file-drop";
import { OrgSwitcher } from "../patterns/org-switcher";
import { SideNav } from "../patterns/side-nav";
import { TopBar } from "../patterns/top-bar";
import { AlertDialog } from "../primitives/alert-dialog";
import { Avatar } from "../primitives/avatar";
import { Breadcrumb, Breadcrumbs } from "../primitives/breadcrumbs";
import { Button } from "../primitives/button";
import { ButtonGroup } from "../primitives/button-group";
import { Checkbox, CheckboxGroup } from "../primitives/checkbox";
import { ComboBox, ComboBoxItem } from "../primitives/combo-box";
import { CopyButton } from "../primitives/copy-button";
import { DataList, DataListItem } from "../primitives/data-list";
import { DatePicker } from "../primitives/date-picker";
import { DateRangePicker } from "../primitives/date-range-picker";
import { Dialog } from "../primitives/dialog";
import { Heading } from "../primitives/heading";
import { IconButton } from "../primitives/icon-button";
import { Kbd } from "../primitives/kbd";
import { Link } from "../primitives/link";
import { Menu, MenuItem, MenuTrigger } from "../primitives/menu";
import { Meter } from "../primitives/meter";
import { NumberField } from "../primitives/number-field";
import { Pagination } from "../primitives/pagination";
import { Popover, PopoverDialog, PopoverTrigger } from "../primitives/popover";
import { ProgressBar } from "../primitives/progress-bar";
import { Prose } from "../primitives/prose";
import { Radio, RadioGroup } from "../primitives/radio-group";
import { SearchField } from "../primitives/search-field";
import { Select, SelectItem } from "../primitives/select";
import { Separator } from "../primitives/separator";
import { Sheet } from "../primitives/sheet";
import { Switch } from "../primitives/switch";
import { Tab, TabList, TabPanel, Tabs } from "../primitives/tabs";
import { Text } from "../primitives/text";
import { TextArea } from "../primitives/text-area";
import { TextField } from "../primitives/text-field";
import { TimeField } from "../primitives/time-field";
import { createToastQueue, ToastRegion } from "../primitives/toast";
import { ToggleButton, ToggleButtonGroup } from "../primitives/toggle-button";
import { Toolbar } from "../primitives/toolbar";
import { Tooltip, TooltipTrigger } from "../primitives/tooltip";
import { Tree, TreeItem } from "../primitives/tree";
import { VisuallyHidden } from "../primitives/visually-hidden";
import { renderWithStrings, unmarkedText } from "../test/strings";
import { Icon } from "../theme/icon";
import { ThemeProvider } from "../theme/theme-provider";

const navEntries = [{ id: "a", href: "/a", label: "«caller»", icon: HouseIcon }];

const toasts = createToastQueue();
toasts.add({ title: "«caller»", description: "«caller»", tone: "error" });

const tableColumns = [
  { id: "a", label: "«caller»", allowsSorting: true, render: () => "«caller»" },
  { id: "b", label: "«caller»", align: "end" as const, render: () => "«caller»" },
];

const components: [string, ReactElement][] = [
  [
    "AlertDialog (open)",
    <AlertDialog key="ad" defaultOpen title="«caller»" confirmLabel="«caller»" onConfirm={() => {}}>
      «caller»
    </AlertDialog>,
  ],
  [
    "AppShell",
    <AppShell
      key="as"
      sideNav={<SideNav entries={navEntries} />}
      bottomTabs={<BottomTabs entries={navEntries} />}
      topBar={<TopBar title="«caller»" />}
    >
      «caller»
    </AppShell>,
  ],
  [
    "Breadcrumbs",
    <Breadcrumbs key="bc">
      <Breadcrumb href="/a">«caller»</Breadcrumb>
      <Breadcrumb>«caller»</Breadcrumb>
    </Breadcrumbs>,
  ],
  [
    "Checkbox",
    <Checkbox key="cx" isInvalid description="«caller»" errorMessage="«caller»">
      «caller»
    </Checkbox>,
  ],
  [
    "CheckboxGroup",
    <CheckboxGroup key="cg" label="«caller»">
      <Checkbox value="a">«caller»</Checkbox>
    </CheckboxGroup>,
  ],
  [
    "ComboBox (open, loading)",
    <ComboBox key="co" label="«caller»" defaultInputValue="«caller»" isLoading>
      <ComboBoxItem id="a">«caller»</ComboBoxItem>
    </ComboBox>,
  ],
  [
    "CommandPalette (open)",
    <CommandPalette
      key="cp"
      defaultOpen
      commands={[{ id: "a", label: "«caller»" }]}
      onAction={() => {}}
    />,
  ],
  [
    "CommandPalette (open, no match)",
    <CommandPalette key="cq" defaultOpen commands={[]} onAction={() => {}} />,
  ],
  [
    "DataList (selectable)",
    <DataList key="dli" aria-label="«caller»" selectionMode="multiple">
      <DataListItem id="a">«caller»</DataListItem>
    </DataList>,
  ],
  [
    "DataTable (sortable, selectable, paged)",
    <DataTable
      key="dt"
      aria-label="«caller»"
      columns={tableColumns}
      rows={[{ id: "r" }]}
      sortDescriptor={{ column: "a", direction: "ascending" }}
      selectionMode="multiple"
      pagination={{ nextCursor: "n", onPageChange: () => {} }}
    />,
  ],
  [
    "DataTable (empty)",
    <DataTable
      key="de"
      aria-label="«caller»"
      columns={tableColumns}
      rows={[]}
      emptyState="«caller»"
    />,
  ],
  [
    "Dialog (open)",
    <Dialog key="dl" defaultOpen title="«caller»">
      «caller»
    </Dialog>,
  ],
  [
    "OrgSwitcher",
    <OrgSwitcher
      key="os"
      organizations={[{ id: "a", name: "«caller»" }]}
      current="a"
      onChange={() => {}}
    />,
  ],
  [
    "Pagination",
    <Pagination key="pg" previousCursor="p" nextCursor="n" count={3} onPageChange={() => {}} />,
  ],
  [
    "RadioGroup",
    <RadioGroup key="rg" label="«caller»">
      <Radio value="a">«caller»</Radio>
    </RadioGroup>,
  ],
  [
    "Select",
    <Select key="se" label="«caller»">
      <SelectItem id="a">«caller»</SelectItem>
    </Select>,
  ],
  [
    "Select (open)",
    <Select key="so" label="«caller»" defaultOpen defaultValue="a">
      <SelectItem id="a">«caller»</SelectItem>
    </Select>,
  ],
  [
    "Sheet (open)",
    <Sheet key="sh" defaultOpen title="«caller»" footer="«caller»">
      «caller»
    </Sheet>,
  ],
  ["SideNav (collapsed)", <SideNav key="sn" entries={navEntries} collapsed />],
  ["Switch", <Switch key="sw">«caller»</Switch>],
  [
    "Tabs",
    <Tabs key="tb">
      <TabList aria-label="«caller»">
        <Tab id="a">«caller»</Tab>
      </TabList>
      <TabPanel id="a">«caller»</TabPanel>
    </Tabs>,
  ],
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
  ["Prose (HTML)", <Prose key="ph" html="<p>«caller»</p>" />],
  ["SearchField", <SearchField key="sf" label="«caller»" defaultValue="«caller»" />],
  ["Separator", <Separator key="s" />],
  ["Text", <Text key="t">«caller»</Text>],
  ["Toast (one shown)", <ToastRegion key="to" queue={toasts} />],
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
  [
    "FileDrop (uploading, with an error)",
    <FileDrop
      key="fd"
      onSelect={() => {}}
      maxFileSize={1}
      description="«caller»"
      upload={{ name: "«caller»", fraction: 0.5 }}
      errorMessage="«caller»"
    />,
  ],
  ["StatusChip", <StatusChip key="sc" state="complete" />],
  [
    "DueBadge (overdue)",
    <DueBadge
      key="db"
      due={new Date("2026-03-07T14:00:00Z")}
      now={new Date("2026-03-10T14:00:00Z")}
      timeZone="UTC"
    />,
  ],
  [
    "RequirementLabel",
    <RequirementLabel key="rl" requirement={{ kind: "watch", min_percent: 80 }} />,
  ],
  [
    "ProgressRail (every state and lock reason)",
    <ProgressRail
      key="pr"
      aria-label="«caller»"
      timeZone="UTC"
      now={new Date("2026-03-10T14:00:00Z")}
      currentId="b"
      items={[
        { id: "a", title: "«caller»", href: "/a", state: "complete" },
        { id: "b", title: "«caller»", href: "/b", state: "available" },
        { id: "c", title: "«caller»", href: "/c", state: "progress" },
        { id: "d", title: "«caller»", href: "/d", state: "review" },
        { id: "e", title: "«caller»", href: "/e", state: "overdue", due: new Date(0) },
        {
          id: "f",
          title: "«caller»",
          href: "/f",
          state: "locked",
          lockReasons: [
            { kind: "unlock_at", at: new Date(0) },
            { kind: "prerequisites", moduleTitles: ["«caller»"] },
            { kind: "module_locked" },
            { kind: "sequential", itemTitle: "«caller»" },
          ],
        },
      ]}
    />,
  ],
  [
    "NextUp (next)",
    <NextUp
      key="nu"
      courseTitle="«caller»"
      progress={{ completed: 1, required: 2 }}
      timeZone="UTC"
      status={{
        kind: "next",
        itemTitle: "«caller»",
        moduleTitle: "«caller»",
        href: "/a",
        started: true,
      }}
    />,
  ],
  [
    "NextUp (waiting)",
    <NextUp key="nw" timeZone="UTC" status={{ kind: "waiting", until: new Date(0) }} />,
  ],
  [
    "Callout",
    <Callout key="co" kind="warning" title="«caller»">
      «caller»
    </Callout>,
  ],
  [
    "Announcement",
    <Announcement
      key="an"
      title="«caller»"
      author="«caller»"
      publishedAt={new Date(0)}
      timeZone="UTC"
      html="<p>«caller»</p>"
    />,
  ],
  [
    "Tree (open)",
    <Tree key="tr" aria-label="«caller»" defaultExpandedKeys={["a"]}>
      <TreeItem id="a" title="«caller»">
        <TreeItem id="b" title="«caller»" />
      </TreeItem>
    </Tree>,
  ],
];

const due = parseZonedDateTime("2026-03-08T09:00[America/New_York]");

/** The date fields: their segments and calendar show values Intl formats, not messages. */
const dateFields: [string, ReactElement][] = [
  [
    "DatePicker (open)",
    <DatePicker key="dp" label="«caller»" timeZone="UTC" defaultValue={due} defaultOpen />,
  ],
  [
    "DateRangePicker (open)",
    <DateRangePicker
      key="dr"
      label="«caller»"
      timeZone="UTC"
      defaultValue={{ start: due, end: due.add({ days: 2 }) }}
      defaultOpen
    />,
  ],
  [
    "TimeField",
    <TimeField
      key="tf"
      label="«caller»"
      timeZone="UTC"
      defaultValue={due}
      description="«caller»"
    />,
  ],
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

  // React Aria's own accessibility strings, which it translates and offers no prop to replace
  // (spec 13 B5): the hidden "Dismiss" button at each end of a modal overlay, and a table's
  // "sortable column" description of a sortable header and "sorted by column … in … order"
  // description of the table, with the column's name taken from the caller. They are the only text
  // a component shows that is not a Stepclimb message.
  const reactAria = [
    /^Dismiss$/,
    /^sortablecolumn$/,
    /^sortedbycolumnin(ascending|descending)order$/,
  ];
  const notReactAria = (text: string) => !reactAria.some((pattern) => pattern.test(text));

  it.each(components)("%s renders only catalog messages and the caller's content", (_, ui) => {
    renderWithStrings(ui, { marked: true });
    // The whole body: overlays render outside the container.
    expect(unmarkedText(document.body).filter(notReactAria)).toEqual([]);
  });

  // The segments (digits, "AM", the zone's short name, "mm" while empty), the calendar's month
  // heading, weekday initials and day numbers, and each day's full name are the locale's
  // formatting of the value through Intl, which React Aria does. React Aria also adds, with no
  // prop to replace them: the calendar's name and a visually hidden heading repeating the visible
  // month, a hidden "Next" button for touch screen readers after the grid, and descriptions of the
  // selected value ("Selected Date: …", "Selected Time: …", "Selected Range: …") and of how to
  // start a range.
  const intlFormatted = [
    '[data-slot="date-segment"]',
    '[data-slot="calendar-heading"]',
    '[data-slot="calendar-grid"]',
  ];
  const reactAriaDates = [
    /^[A-Z][a-z]+\d{4}$/,
    /^Next$/,
    /^Selected(Date|Time|Range)/,
    /^Clicktostartselectingdaterange$/,
  ];
  const notReactAriaDates = (text: string) =>
    notReactAria(text) && !reactAriaDates.some((pattern) => pattern.test(text));
  it.each(dateFields)("%s renders only catalog messages outside Intl's values", (_, ui) => {
    renderWithStrings(ui, { marked: true });
    const copy = document.body.cloneNode(true) as HTMLElement;
    for (const el of copy.querySelectorAll(intlFormatted.join(", "))) el.remove();
    expect(unmarkedText(copy).filter(notReactAriaDates)).toEqual([]);
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
