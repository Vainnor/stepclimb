// SPDX-License-Identifier: AGPL-3.0-or-later
// OrgSwitcher: moves between the organizations a person is signed in to
// (docs/specs/m0/13-base-ui-components.md M0 inventory; spec 11 B19, Q3). The app renders it only in
// multi-organization mode; in single-organization mode it is not rendered at all. A Menu of the
// organizations, the current one checked; choosing another calls `onChange`, and the app goes to
// that organization's origin.
import { CaretUpDownIcon } from "@phosphor-icons/react";
import type { Key } from "react-aria-components";
import { tv } from "../lib/tv";
import { Button } from "../primitives/button";
import { Menu, MenuItem, MenuTrigger } from "../primitives/menu";
import { useStepclimbStrings } from "../strings/strings";
import { Icon } from "../theme/icon";

export const orgSwitcher = tv({
  slots: {
    trigger: "max-w-full justify-between",
    name: "min-w-0 truncate",
    caret: "shrink-0",
  },
});

const styles = orgSwitcher();

export interface Organization {
  /** Unique among the organizations. */
  id: string;
  /** The organization's display name. */
  name: string;
}

export interface OrgSwitcherProps {
  /** The organizations the browser holds sessions for. */
  organizations: readonly Organization[];
  /** The `id` of the organization of this page. */
  current: string;
  /** Called with the `id` of another organization the person chose. */
  onChange: (id: string) => void;
  /** Layout only, on the button. */
  className?: string;
}

export function OrgSwitcher({ organizations, current, onChange, className }: OrgSwitcherProps) {
  const { t } = useStepclimbStrings();
  const name = organizations.find((org) => org.id === current)?.name ?? "";
  const choose = (keys: "all" | Set<Key>) => {
    if (keys === "all") return;
    // The selection cannot be emptied, so choosing the current organization changes nothing and
    // React Aria does not call this.
    const [id] = keys;
    if (typeof id === "string") onChange(id);
  };
  return (
    <MenuTrigger>
      <Button
        data-slot="org-switcher"
        intent="ghost"
        aria-label={t("orgSwitcher.label", { name })}
        className={styles.trigger({ className })}
      >
        <span data-slot="org-switcher-name" className={styles.name()}>
          {name}
        </span>
        <Icon icon={CaretUpDownIcon} size="sm" className={styles.caret()} />
      </Button>
      <Menu
        selectionMode="single"
        disallowEmptySelection
        selectedKeys={[current]}
        onSelectionChange={choose}
        items={organizations}
      >
        {(org) => (
          <MenuItem id={org.id} data-slot="org-switcher-item">
            {org.name}
          </MenuItem>
        )}
      </Menu>
    </MenuTrigger>
  );
}
