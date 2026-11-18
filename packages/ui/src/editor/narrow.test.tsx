// SPDX-License-Identifier: AGPL-3.0-or-later
// Under 768 px "Insert" opens the block list as a bottom sheet (Menu's narrow form).
import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { stubMatchMedia } from "../test/media";
import { renderWithStrings } from "../test/strings";
import { Editor } from "./editor";

afterEach(() => vi.unstubAllGlobals());

test("Insert opens the block list as a bottom sheet on a narrow screen", async () => {
  stubMatchMedia(() => false);
  renderWithStrings(<Editor />);
  const user = userEvent.setup();
  await user.click(await screen.findByRole("button", { name: "Insert" }));
  const sheet = document.querySelector<HTMLElement>('[data-slot="menu-sheet"]');
  expect(sheet).not.toBeNull();
  expect(
    within(sheet as HTMLElement).getByRole("menuitem", { name: "Callout" }),
  ).toBeInTheDocument();
});
