// SPDX-License-Identifier: AGPL-3.0-or-later
// `/account`, the signed-in user's account page (docs/specs/m0/11-app-shell.md Q2;
// 04-local-accounts.md B13): in M0, the user's passkeys. The password and TOTP of 04 I6 and I7
// join it as their own sections.
import { PasskeySection } from "../account/passkey-section";

export function AccountView() {
  return (
    <div className="flex flex-col gap-8 p-4 md:p-6">
      <PasskeySection />
    </div>
  );
}
