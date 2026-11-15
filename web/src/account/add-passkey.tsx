// SPDX-License-Identifier: AGPL-3.0-or-later
// Adding a passkey from the account page (docs/specs/m0/04-local-accounts.md B13): a dialog with
// an optional name. Opening it asks the server for the creation options, so its button goes
// straight to the browser's passkey prompt, which some browsers allow only right after a press.
// The credential goes to the add route with the name; the server checks both. When the server has
// deleted the registration's flow (expired, used, or the passkey refused), the dialog asks for new
// options. A refusal that cannot change by trying again (passkeys off for the organization, or a
// provider that does not allow them) leaves only Cancel.
import {
  Button,
  Dialog,
  DialogBody,
  DialogFooter,
  DialogTrigger,
  FormError,
  Text,
  TextField,
  useStepclimbStrings,
} from "@stepclimb/ui";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { Form } from "react-aria-components";
import { useAppContext } from "../context";
import { needsNewCeremony } from "../sign-in/problems";
import { addPasskey, type MyPasskey, myPasskeysQuery, startAddPasskey } from "./passkeys";
import { isFinalAddRefusal, nameError, passkeyProblem } from "./problems";

/** The "Add a passkey" button and the dialog it opens. */
export function AddPasskey() {
  const { t } = useStepclimbStrings();
  return (
    <DialogTrigger>
      <Button intent="primary">{t("account.passkeys.add")}</Button>
      <Dialog title={t("account.passkeys.add.title")}>
        {({ close }) => <AddPasskeyForm onDone={close} />}
      </Dialog>
    </DialogTrigger>
  );
}

function useAddPasskey(onAdded: (passkey: MyPasskey) => void) {
  // A POST that starts a flow: fetched once each time the dialog opens, never again on focus or
  // reconnect, and dropped when it closes.
  const start = useQuery({
    queryKey: ["me", "passkeys", "start"],
    queryFn: startAddPasskey,
    staleTime: Number.POSITIVE_INFINITY,
    gcTime: 0,
    refetchOnWindowFocus: false,
    refetchOnReconnect: false,
  });
  const add = useMutation({
    mutationFn: ({ name }: { name: string }) => {
      if (!start.data) throw new Error("adding a passkey before its options arrived");
      return addPasskey(start.data, name);
    },
    onSuccess: onAdded,
    onError: (error) => {
      if (needsNewCeremony(error)) void start.refetch();
    },
  });
  return { start, add };
}

function AddPasskeyForm({ onDone }: { onDone: () => void }) {
  const { t } = useStepclimbStrings();
  const { toasts } = useAppContext();
  const queryClient = useQueryClient();
  const [name, setName] = useState("");
  const { start, add } = useAddPasskey((passkey) => {
    queryClient.setQueryData<MyPasskey[]>(myPasskeysQuery.queryKey, (list) =>
      list === undefined ? list : [...list, passkey],
    );
    toasts.add({ tone: "success", title: t("account.passkeys.added", { name: passkey.name }) });
    onDone();
  });

  const failure = add.error ?? (start.isError ? start.error : null);
  const fieldError = nameError(add.error, t);
  const problem = failure && fieldError === undefined ? passkeyProblem(failure, t) : undefined;
  const final = start.isError && isFinalAddRefusal(start.error);
  const busy = add.isPending || start.isFetching || add.isSuccess;
  // Without options (the start failed), the button asks for them again.
  const retry = start.isError && !start.isFetching;
  const submit = () => {
    // Enter in the name field submits too; one registration at a time.
    if (busy) return;
    add.reset();
    if (start.data) add.mutate({ name });
    else void start.refetch();
  };

  return (
    <Form
      onSubmit={(event) => {
        event.preventDefault();
        submit();
      }}
      validationErrors={fieldError === undefined ? {} : { name: fieldError }}
      className="flex flex-col gap-4"
    >
      <DialogBody>
        <Text elementType="p">{t("account.passkeys.add.description")}</Text>
        {problem ? <FormError code={problem.code}>{problem.message}</FormError> : null}
        {final ? null : (
          <TextField
            label={t("account.passkeys.name.label")}
            description={t("account.passkeys.add.nameDescription")}
            name="name"
            value={name}
            onChange={setName}
          />
        )}
      </DialogBody>
      <DialogFooter>
        <Button onPress={onDone}>{t("account.cancel")}</Button>
        {final ? null : (
          <Button type="submit" intent="primary" isPending={busy}>
            {retry ? t("loadError.retry") : t("account.passkeys.add.submit")}
          </Button>
        )}
      </DialogFooter>
    </Form>
  );
}
