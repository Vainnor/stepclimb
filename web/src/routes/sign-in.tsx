// SPDX-License-Identifier: AGPL-3.0-or-later
// `/sign-in`, the email step (docs/specs/m0/11-app-shell.md B13, B15, B16): local sign-in is email
// first (04-local-accounts Q9), then the passkey step at `/sign-in/passkey`. The address travels to
// that step in the history entry's state, never in the URL, so it stays out of server logs and a
// refresh of the passkey step still has it; this step's own entry keeps it too, so Back finds the
// field filled in. The return target rides along in the search.
import { Button, Heading, TextField, useStepclimbStrings } from "@stepclimb/ui";
import { useLocation, useNavigate, useSearch } from "@tanstack/react-router";
import { type FormEvent, useState } from "react";
import { Form } from "react-aria-components";
import { useDocumentTitle } from "../layouts/document-title";

export function SignInView() {
  const { t } = useStepclimbStrings();
  const title = t("signIn.title");
  useDocumentTitle(title);
  const search = useSearch({ from: "/signed-out/sign-in" });
  const remembered = useLocation({ select: (location) => location.state.email });
  const navigate = useNavigate();
  const [email, setEmail] = useState(remembered ?? "");

  const submit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const state = { email: email.trim() };
    await navigate({ to: "/sign-in", search, state, replace: true });
    await navigate({ to: "/sign-in/passkey", search, state });
  };

  return (
    <>
      <Heading level={1}>{title}</Heading>
      <Form onSubmit={(event) => void submit(event)} className="flex flex-col gap-4">
        <TextField
          label={t("signIn.email.label")}
          type="email"
          name="email"
          autoComplete="username"
          inputMode="email"
          isRequired
          value={email}
          onChange={setEmail}
        />
        <Button type="submit" intent="primary">
          {t("signIn.continue")}
        </Button>
      </Form>
    </>
  );
}
