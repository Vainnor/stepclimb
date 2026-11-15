// SPDX-License-Identifier: AGPL-3.0-or-later
// Renaming a passkey (docs/specs/m0/04-local-accounts.md B13): a dialog with the name filled in.
// The server trims and checks the name; a name it refuses is shown at the field, any other refusal
// above it. A passkey that is gone (deleted in another tab) refreshes the list behind the dialog.
import {
  Button,
  Dialog,
  DialogBody,
  DialogFooter,
  DialogTrigger,
  FormError,
  TextField,
  useStepclimbStrings,
} from "@stepclimb/ui";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { Form } from "react-aria-components";
import { type MyPasskey, myPasskeysQuery, renamePasskey } from "./passkeys";
import { isPasskeyGone, nameError, passkeyProblem } from "./problems";

/** The "Rename" button of one passkey and the dialog it opens. */
export function RenamePasskey({ passkey }: { passkey: MyPasskey }) {
  const { t } = useStepclimbStrings();
  return (
    <DialogTrigger>
      <Button size="sm" aria-label={t("account.passkeys.renameLabel", { name: passkey.name })}>
        {t("account.passkeys.rename")}
      </Button>
      <Dialog title={t("account.passkeys.rename.title")}>
        {({ close }) => <RenameForm passkey={passkey} onDone={close} />}
      </Dialog>
    </DialogTrigger>
  );
}

function RenameForm({ passkey, onDone }: { passkey: MyPasskey; onDone: () => void }) {
  const { t } = useStepclimbStrings();
  const queryClient = useQueryClient();
  const [name, setName] = useState(passkey.name);
  const rename = useMutation({
    mutationFn: () => renamePasskey(passkey.id, name),
    onSuccess: (renamed) => {
      queryClient.setQueryData<MyPasskey[]>(myPasskeysQuery.queryKey, (list) =>
        list?.map((item) => (item.id === renamed.id ? renamed : item)),
      );
      onDone();
    },
    onError: (error) => {
      if (isPasskeyGone(error)) void queryClient.invalidateQueries(myPasskeysQuery);
    },
  });
  const fieldError = nameError(rename.error, t);
  const problem =
    rename.error && fieldError === undefined ? passkeyProblem(rename.error, t) : undefined;

  return (
    <Form
      onSubmit={(event) => {
        event.preventDefault();
        // Enter in the name field submits too; one rename at a time.
        if (rename.isPending) return;
        rename.reset();
        rename.mutate();
      }}
      validationErrors={fieldError === undefined ? {} : { name: fieldError }}
      className="flex flex-col gap-4"
    >
      <DialogBody>
        {problem ? <FormError code={problem.code}>{problem.message}</FormError> : null}
        <TextField
          label={t("account.passkeys.name.label")}
          name="name"
          value={name}
          onChange={setName}
          autoFocus
        />
      </DialogBody>
      <DialogFooter>
        <Button onPress={onDone}>{t("account.cancel")}</Button>
        <Button type="submit" intent="primary" isPending={rename.isPending}>
          {t("account.passkeys.rename.submit")}
        </Button>
      </DialogFooter>
    </Form>
  );
}
