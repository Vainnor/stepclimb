// SPDX-License-Identifier: AGPL-3.0-or-later
// The user menu at the TopBar's inline end (docs/specs/m0/11-app-shell.md B4, B12, B21): the
// user's Avatar and display name, opening a menu with the theme and density choices and Sign out.
// Until the user chooses a theme, the menu marks the organization's default appearance, and a
// density change keeps the theme unchosen.
// A choice is saved on the server first, then applied to the page; a failed save shows an error
// toast and leaves the page as it was. While a save is pending, both submenus are disabled and the
// one being saved says so, so a second choice cannot send the first one's old value back. Signing out calls the server, then leaves the signed-in
// area; a failed sign-out shows an error toast and leaves the user signed in.
import {
  Avatar,
  Button,
  Menu,
  MenuItem,
  MenuTrigger,
  type Strings,
  SubmenuTrigger,
  useStepclimbStrings,
  VisuallyHidden,
} from "@stepclimb/ui";
import { useMutation } from "@tanstack/react-query";
import type { Key } from "react-aria-components";
import {
  applyAppearance,
  type DensityChoice,
  densityChoices,
  saveAppearance,
  shownTheme,
  type ThemeChoice,
  themeChoices,
  type UserAppearance,
} from "../appearance";
import { useAppContext } from "../context";
import { clearOwnerDrafts, draftOwner } from "../pages/draft";
import { type Me, meQuery, signOut } from "../session";
import { clearVideoUploads } from "../videos/videos";

function themeLabel(t: Strings["t"], theme: ThemeChoice): string {
  const labels: Record<ThemeChoice, string> = {
    system: t("appearance.theme.system"),
    light: t("appearance.theme.light"),
    dark: t("appearance.theme.dark"),
    "high-contrast": t("appearance.theme.highContrast"),
  };
  return labels[theme];
}

function densityLabel(t: Strings["t"], density: DensityChoice): string {
  const labels: Record<DensityChoice, string> = {
    comfortable: t("appearance.density.comfortable"),
    compact: t("appearance.density.compact"),
  };
  return labels[density];
}

/** The one key of a single selection, or undefined for "all" or an empty set. */
function chosen(keys: "all" | Set<Key>): string | undefined {
  if (keys === "all") return undefined;
  const [key] = keys;
  return typeof key === "string" ? key : undefined;
}

function useAppearanceChange(organizationDefault: ThemeChoice) {
  const { t } = useStepclimbStrings();
  const { toasts, queryClient } = useAppContext();
  return useMutation({
    mutationFn: saveAppearance,
    onSuccess: (saved) => {
      applyAppearance(shownTheme(saved, organizationDefault), saved.density);
      queryClient.setQueryData<Me>(meQuery.queryKey, (me) => me && { ...me, appearance: saved });
    },
    onError: () => {
      toasts.add({
        tone: "error",
        title: t("appearance.failed"),
        description: t("appearance.failedHint"),
      });
    },
  });
}

export interface UserMenuProps {
  name: string;
  appearance: UserAppearance;
  /** The organization's default appearance, shown while `appearance.theme` is null. */
  organizationDefault: ThemeChoice;
}

export function UserMenu({ name, appearance, organizationDefault }: UserMenuProps) {
  const { t } = useStepclimbStrings();
  const { toasts, leaveSession, queryClient } = useAppContext();
  const change = useAppearanceChange(organizationDefault);
  const signOutMutation = useMutation({
    mutationFn: signOut,
    onSuccess: () => {
      // An explicit sign-out takes the user's unfinished video uploads (spec 08 D30) and unsaved
      // page work (spec 07 B4) with it; a session that expired keeps both for when they sign in
      // again.
      const me = queryClient.getQueryData(meQuery.queryKey);
      if (me) {
        clearVideoUploads(me);
        clearOwnerDrafts(draftOwner(me));
      }
      return leaveSession();
    },
    onError: () => {
      toasts.add({
        tone: "error",
        title: t("signOut.failed"),
        description: t("signOut.failedHint"),
      });
    },
  });
  // The choice on its way to the server, shown in place of the saved one until it answers.
  const pending = change.isPending ? change.variables : undefined;
  const shown = pending ?? appearance;
  const currentTheme = shownTheme(shown, organizationDefault);
  const savingTheme = pending && pending.theme !== appearance.theme;
  const savingDensity = pending && pending.density !== appearance.density;
  const chooseTheme = (keys: "all" | Set<Key>) => {
    if (change.isPending) return;
    const theme = themeChoices.find((choice) => choice === chosen(keys));
    if (theme) change.mutate({ ...appearance, theme });
  };
  const chooseDensity = (keys: "all" | Set<Key>) => {
    if (change.isPending) return;
    const density = densityChoices.find((choice) => choice === chosen(keys));
    if (density) change.mutate({ ...appearance, density });
  };
  return (
    <MenuTrigger>
      {/* The accessible name is "{name}, account menu", from the hidden text. The Avatar and the
          visible name are decorative and hidden from assistive technology; with an `aria-label`
          instead, axe's label-content-name-mismatch rule reads the initials as label text. */}
      <Button intent="ghost" isPending={signOutMutation.isPending}>
        <span aria-hidden="true" className="flex items-center gap-2">
          <Avatar name={name} size="sm" />
          <span className="max-w-48 truncate max-md:hidden">{name}</span>
        </span>
        <VisuallyHidden>{t("userMenu.label", { name })}</VisuallyHidden>
      </Button>
      <Menu
        placement="bottom end"
        onAction={(key) => {
          if (key === "sign-out") signOutMutation.mutate();
        }}
      >
        <SubmenuTrigger>
          <MenuItem id="theme" isDisabled={change.isPending} textValue={t("userMenu.theme")}>
            {savingTheme
              ? t("userMenu.saving", {
                  setting: t("userMenu.theme"),
                  choice: themeLabel(t, currentTheme),
                })
              : t("userMenu.theme")}
          </MenuItem>
          <Menu
            aria-label={t("userMenu.theme")}
            selectionMode="single"
            disallowEmptySelection
            selectedKeys={[currentTheme]}
            onSelectionChange={chooseTheme}
          >
            {themeChoices.map((theme) => (
              <MenuItem key={theme} id={theme}>
                {themeLabel(t, theme)}
              </MenuItem>
            ))}
          </Menu>
        </SubmenuTrigger>
        <SubmenuTrigger>
          <MenuItem id="density" isDisabled={change.isPending} textValue={t("userMenu.density")}>
            {savingDensity
              ? t("userMenu.saving", {
                  setting: t("userMenu.density"),
                  choice: densityLabel(t, shown.density),
                })
              : t("userMenu.density")}
          </MenuItem>
          <Menu
            aria-label={t("userMenu.density")}
            selectionMode="single"
            disallowEmptySelection
            selectedKeys={[shown.density]}
            onSelectionChange={chooseDensity}
          >
            {densityChoices.map((density) => (
              <MenuItem key={density} id={density}>
                {densityLabel(t, density)}
              </MenuItem>
            ))}
          </Menu>
        </SubmenuTrigger>
        <MenuItem id="sign-out">{t("userMenu.signOut")}</MenuItem>
      </Menu>
    </MenuTrigger>
  );
}
