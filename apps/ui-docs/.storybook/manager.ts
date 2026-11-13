// SPDX-License-Identifier: AGPL-3.0-or-later
// The sidebar names each component's status beside it (docs/specs/m0/13-base-ui-components.md B28).
import { addons } from "storybook/manager-api";
import { statusOf } from "./status";

addons.setConfig({
  sidebar: {
    renderLabel: (item) => {
      if (item.type !== "component") return item.name;
      const status = statusOf(item.tags);
      return status ? `${item.name} (${status})` : item.name;
    },
  },
});
