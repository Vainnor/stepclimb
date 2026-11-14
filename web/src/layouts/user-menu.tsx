// SPDX-License-Identifier: AGPL-3.0-or-later
// The user menu at the TopBar's inline end (docs/specs/m0/11-app-shell.md B4, B12): the user's
// Avatar and display name, opening a menu with Sign out. Signing out calls the server, then leaves
// the signed-in area; a failed sign-out shows an error toast and leaves the user signed in.
import { Avatar, Button, Menu, MenuItem, MenuTrigger, useStepclimbStrings } from "@stepclimb/ui";
import { useMutation } from "@tanstack/react-query";
import { useAppContext } from "../context";
import { signOut } from "../session";

export function UserMenu({ name }: { name: string }) {
  const { t } = useStepclimbStrings();
  const { toasts, leaveSession } = useAppContext();
  const signOutMutation = useMutation({
    mutationFn: signOut,
    onSuccess: () => leaveSession(),
    onError: () => {
      toasts.add({
        tone: "error",
        title: t("signOut.failed"),
        description: t("signOut.failedHint"),
      });
    },
  });
  return (
    <MenuTrigger>
      <Button
        intent="ghost"
        aria-label={t("userMenu.label", { name })}
        isPending={signOutMutation.isPending}
      >
        <Avatar name={name} size="sm" />
        <span className="max-w-48 truncate max-md:sr-only">{name}</span>
      </Button>
      <Menu
        placement="bottom end"
        onAction={(key) => {
          if (key === "sign-out") signOutMutation.mutate();
        }}
      >
        <MenuItem id="sign-out">{t("userMenu.signOut")}</MenuItem>
      </Menu>
    </MenuTrigger>
  );
}
