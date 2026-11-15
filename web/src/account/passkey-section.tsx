// SPDX-License-Identifier: AGPL-3.0-or-later
// The account page's passkeys (docs/specs/m0/04-local-accounts.md B13): the list with each
// passkey's name, when it was added and when it last signed in, Rename and Delete on each, and
// "Add a passkey". Deleting asks first. The server decides what may be deleted: the user's last
// way to sign in is refused with `local.last_method`, which the page explains above the list.
// While passkeys are off for the organization the server refuses every route, and the section
// says so instead of the list.
import { KeyIcon } from "@phosphor-icons/react";
import {
  AlertDialog,
  Button,
  DataList,
  DataListItem,
  DialogTrigger,
  EmptyState,
  FormError,
  Heading,
  Skeleton,
  Text,
  useStepclimbStrings,
} from "@stepclimb/ui";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useId } from "react";
import { useAppContext } from "../context";
import { LoadError } from "../layouts/load-error";
import { PasskeyError, passkeyUnavailable } from "../sign-in/passkey";
import { AddPasskey } from "./add-passkey";
import { deletePasskey, type MyPasskey, myPasskeysQuery } from "./passkeys";
import { isLastMethod, isMethodDisabled, isPasskeyGone, passkeyProblem } from "./problems";
import { RenamePasskey } from "./rename-passkey";

export function PasskeySection() {
  const { t } = useStepclimbStrings();
  const headingId = useId();
  const list = useQuery(myPasskeysQuery);
  const remove = useDeletePasskey(headingId);
  const unavailable = passkeyUnavailable();
  const off = isMethodDisabled(list.error);
  // The last-method refusal is about the list it was refused against: once the list changes (a
  // passkey added, renamed or deleted), it no longer holds and goes.
  const removeError =
    isLastMethod(remove.error) && remove.variables?.shown !== list.data ? null : remove.error;

  return (
    <section aria-labelledby={headingId} className="flex max-w-3xl flex-col gap-4">
      <div className="flex flex-wrap items-end justify-between gap-3">
        <div className="flex min-w-0 flex-col gap-1">
          <Heading level={2} size="xl" id={headingId} tabIndex={-1}>
            {t("account.passkeys.title")}
          </Heading>
          <Text elementType="p" tone="sub">
            {t("account.passkeys.description")}
          </Text>
        </div>
        {list.data && unavailable === undefined ? <AddPasskey /> : null}
      </div>
      {list.data && unavailable ? (
        <Text elementType="p">{passkeyProblem(new PasskeyError(unavailable), t).message}</Text>
      ) : null}
      {removeError ? <Problem error={removeError} /> : null}
      {off ? <Text elementType="p">{passkeyProblem(list.error, t).message}</Text> : null}
      {list.isError && !off ? (
        <LoadError error={list.error} level={3} onRetry={() => void list.refetch()} />
      ) : null}
      {list.isPending ? <ListSkeleton /> : null}
      {list.data ? (
        <PasskeyList
          passkeys={list.data}
          labelledBy={headingId}
          deleting={remove.isPending ? remove.variables?.passkey.id : undefined}
          onDelete={(passkey) => {
            remove.reset();
            remove.mutate({ passkey, shown: list.data });
          }}
        />
      ) : null}
    </section>
  );
}

interface Removal {
  passkey: MyPasskey;
  /** The list on screen when the user asked, which a last-method refusal is about. */
  shown: MyPasskey[] | undefined;
}

/** Deletes a passkey; on success the list drops it, a toast says so and focus goes to the heading. */
function useDeletePasskey(headingId: string) {
  const { t } = useStepclimbStrings();
  const { toasts } = useAppContext();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ passkey }: Removal) => deletePasskey(passkey.id),
    onSuccess: (_, { passkey }) => {
      queryClient.setQueryData<MyPasskey[]>(myPasskeysQuery.queryKey, (list) =>
        list?.filter((item) => item.id !== passkey.id),
      );
      toasts.add({ tone: "success", title: t("account.passkeys.deleted", { name: passkey.name }) });
      // The item that held focus is gone; the section's heading keeps the place.
      document.getElementById(headingId)?.focus();
    },
    onError: (error) => {
      if (isPasskeyGone(error)) void queryClient.invalidateQueries(myPasskeysQuery);
    },
  });
}

function Problem({ error }: { error: unknown }) {
  const { t } = useStepclimbStrings();
  const problem = passkeyProblem(error, t);
  return <FormError code={problem.code}>{problem.message}</FormError>;
}

interface PasskeyListProps {
  passkeys: MyPasskey[];
  labelledBy: string;
  /** The id of the passkey being deleted. */
  deleting: string | undefined;
  onDelete: (passkey: MyPasskey) => void;
}

function PasskeyList({ passkeys, labelledBy, deleting, onDelete }: PasskeyListProps) {
  const { t } = useStepclimbStrings();
  return (
    <DataList
      aria-labelledby={labelledBy}
      items={passkeys}
      emptyState={<EmptyState icon={KeyIcon}>{t("account.passkeys.empty")}</EmptyState>}
    >
      {(passkey) => (
        <DataListItem id={passkey.id} textValue={passkey.name}>
          <div className="flex min-w-0 flex-1 flex-wrap items-center justify-between gap-x-4 gap-y-2">
            <PasskeyDetails passkey={passkey} />
            <div className="flex shrink-0 gap-2">
              <RenamePasskey passkey={passkey} />
              <DeletePasskey
                passkey={passkey}
                isPending={deleting === passkey.id}
                onConfirm={() => onDelete(passkey)}
              />
            </div>
          </div>
        </DataListItem>
      )}
    </DataList>
  );
}

function PasskeyDetails({ passkey }: { passkey: MyPasskey }) {
  const { t } = useStepclimbStrings();
  const created = new Date(passkey.created_at);
  const lastUsed = passkey.last_used_at === null ? undefined : new Date(passkey.last_used_at);
  return (
    <div className="flex min-w-0 flex-col gap-0.5">
      <Text tone="strong" className="wrap-anywhere">
        {passkey.name}
      </Text>
      <Text size="sm" tone="sub">
        {lastUsed === undefined
          ? t("account.passkeys.unused", { created })
          : t("account.passkeys.used", { created, lastUsed })}
      </Text>
    </div>
  );
}

interface DeletePasskeyProps {
  passkey: MyPasskey;
  isPending: boolean;
  onConfirm: () => void;
}

/** "Delete" and the dialog that asks before anything is sent. */
function DeletePasskey({ passkey, isPending, onConfirm }: DeletePasskeyProps) {
  const { t } = useStepclimbStrings();
  return (
    <DialogTrigger>
      <Button
        size="sm"
        intent="danger"
        isPending={isPending}
        aria-label={t("account.passkeys.deleteLabel", { name: passkey.name })}
      >
        {t("account.passkeys.delete")}
      </Button>
      <AlertDialog
        title={t("account.passkeys.delete.title", { name: passkey.name })}
        confirmLabel={t("account.passkeys.delete.confirm")}
        intent="danger"
        onConfirm={onConfirm}
      >
        {t("account.passkeys.delete.description")}
      </AlertDialog>
    </DialogTrigger>
  );
}

function ListSkeleton() {
  return (
    <div aria-busy="true" className="flex flex-col gap-2">
      <Skeleton shape="block" className="h-14" />
      <Skeleton shape="block" className="h-14" />
    </div>
  );
}
