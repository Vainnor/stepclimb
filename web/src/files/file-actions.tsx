// SPDX-License-Identifier: AGPL-3.0-or-later
// The actions on one row of the files area (docs/specs/m1/06-files.md B10, B11;
// 16-learning-ui-components.md B20): a file can be renamed, moved to another folder of the course
// or deleted; a folder can be renamed, which moves everything in it. Each opens from the row's
// actions menu. Rename and move ask for the new value in a dialog, which shows the server's
// refusal and stays open; delete asks first, and its refusal (a file an item or page still uses)
// comes back as an error toast. A file or folder that is gone refreshes the list.
import { DotsThreeIcon } from "@phosphor-icons/react";
import {
  AlertDialog,
  Button,
  Dialog,
  DialogBody,
  DialogFooter,
  FormError,
  IconButton,
  Menu,
  MenuItem,
  MenuTrigger,
  TextField,
  useStepclimbStrings,
} from "@stepclimb/ui";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { Form } from "react-aria-components";
import { useAppContext } from "../context";
import {
  type CourseFile,
  courseFilesKey,
  deleteCourseFile,
  renameFolder,
  updateCourseFile,
} from "./course-files";
import { childFolder, normalizeTypedFolder, parentOf } from "./folders";
import { filesProblem, isGone, problemSentence, type Subject } from "./problems";

type FileAction = "rename" | "move" | "delete";

/** The actions menu of a file's row and the dialog the chosen action opens. */
export function FileActions({ file, onClosed }: { file: CourseFile; onClosed: () => void }) {
  const { t } = useStepclimbStrings();
  const [open, setOpen] = useState<FileAction>();
  const close = () => {
    setOpen(undefined);
    onClosed();
  };
  return (
    <>
      <MenuTrigger>
        <IconButton icon={DotsThreeIcon} label={t("files.actions", { name: file.name })} />
        <Menu placement="bottom end" onAction={(key) => setOpen(key as FileAction)}>
          <MenuItem id="rename">{t("files.rename")}</MenuItem>
          <MenuItem id="move">{t("files.move")}</MenuItem>
          <MenuItem id="delete">{t("files.delete")}</MenuItem>
        </Menu>
      </MenuTrigger>
      {open === "rename" ? <RenameFile file={file} onClose={close} /> : null}
      {open === "move" ? <MoveFile file={file} onClose={close} /> : null}
      {open === "delete" ? <DeleteFile file={file} onClose={close} /> : null}
    </>
  );
}

/** The actions menu of a folder's row: rename. */
export function FolderActions({
  courseId,
  path,
  name,
  onClosed,
}: {
  courseId: string;
  path: string;
  name: string;
  onClosed: () => void;
}) {
  const { t } = useStepclimbStrings();
  const [open, setOpen] = useState(false);
  return (
    <>
      <MenuTrigger>
        <IconButton icon={DotsThreeIcon} label={t("files.actions", { name })} />
        <Menu placement="bottom end" onAction={() => setOpen(true)}>
          <MenuItem id="rename">{t("files.rename")}</MenuItem>
        </Menu>
      </MenuTrigger>
      {open ? (
        <RenameFolder
          courseId={courseId}
          path={path}
          name={name}
          onClose={() => {
            setOpen(false);
            onClosed();
          }}
        />
      ) : null}
    </>
  );
}

/**
 * A mutation that refreshes the course's files when it works, or when what it named is gone. Its
 * toasts are sent from `change`, which runs to its end even when the dialog that started it has
 * closed.
 */
function useFilesChange<V>(courseId: string, change: (value: V) => Promise<void>) {
  const queryClient = useQueryClient();
  const refresh = () => queryClient.invalidateQueries({ queryKey: courseFilesKey(courseId) });
  return useMutation({
    mutationFn: change,
    onSuccess: () => void refresh(),
    onError: (error) => {
      if (isGone(error)) void refresh();
    },
  });
}

interface ValueDialogProps {
  title: string;
  label: string;
  description?: string;
  submit: string;
  initial: string;
  /** The name a refusal of `value` is about. */
  subjectOf: (value: string) => string;
  /** Whether the value is for a file or a folder, which a name clash is worded for. */
  kind?: Subject;
  courseId: string;
  /** Sends the value; resolves once the server took it. */
  change: (value: string) => Promise<void>;
  onClose: () => void;
}

/** A dialog asking for one value, sending it with `mutation`, and closing once it worked. */
function ValueDialog(props: ValueDialogProps) {
  const { t } = useStepclimbStrings();
  const { onClose } = props;
  const mutation = useFilesChange(props.courseId, props.change);
  const [value, setValue] = useState(props.initial);
  const problem = mutation.error
    ? filesProblem(mutation.error, props.subjectOf(mutation.variables ?? value), t, props.kind)
    : undefined;
  return (
    <Dialog title={props.title} isOpen onOpenChange={(isOpen) => (isOpen ? null : onClose())}>
      <Form
        onSubmit={(event) => {
          event.preventDefault();
          if (mutation.isPending) return;
          mutation.reset();
          mutation.mutate(value, { onSuccess: onClose });
        }}
        className="flex flex-col gap-4"
      >
        <DialogBody>
          {problem ? <FormError code={problem.code}>{problem.message}</FormError> : null}
          <TextField
            label={props.label}
            description={props.description}
            value={value}
            onChange={setValue}
            isRequired
            autoFocus
          />
        </DialogBody>
        <DialogFooter>
          <Button onPress={onClose}>{t("files.cancel")}</Button>
          <Button type="submit" intent="primary" isPending={mutation.isPending}>
            {props.submit}
          </Button>
        </DialogFooter>
      </Form>
    </Dialog>
  );
}

function RenameFile({ file, onClose }: { file: CourseFile; onClose: () => void }) {
  const { t } = useStepclimbStrings();
  const { toasts } = useAppContext();
  const change = async (name: string) => {
    const renamed = await updateCourseFile(file.id, { name });
    toasts.add({ tone: "success", title: t("files.renamed", { name: renamed.name }) });
  };
  return (
    <ValueDialog
      title={t("files.rename.title", { name: file.name })}
      label={t("files.rename.label")}
      submit={t("files.rename.submit")}
      initial={file.name}
      subjectOf={(name) => name.trim()}
      courseId={file.course_id}
      change={change}
      onClose={onClose}
    />
  );
}

function MoveFile({ file, onClose }: { file: CourseFile; onClose: () => void }) {
  const { t } = useStepclimbStrings();
  const { toasts } = useAppContext();
  const change = async (typed: string) => {
    const moved = await updateCourseFile(file.id, { folder: normalizeTypedFolder(typed) });
    toasts.add({
      tone: "success",
      title: t("files.moved", { name: moved.name, folder: moved.folder }),
    });
  };
  return (
    <ValueDialog
      title={t("files.move.title", { name: file.name })}
      label={t("files.move.label")}
      description={t("files.move.description")}
      submit={t("files.move.submit")}
      initial={file.folder}
      subjectOf={() => file.name}
      courseId={file.course_id}
      change={change}
      onClose={onClose}
    />
  );
}

interface RenameFolderProps {
  courseId: string;
  path: string;
  name: string;
  onClose: () => void;
}

function RenameFolder({ courseId, path, name, onClose }: RenameFolderProps) {
  const { t } = useStepclimbStrings();
  const { toasts } = useAppContext();
  const change = async (typed: string) => {
    const newName = typed.trim();
    await renameFolder(courseId, path, childFolder(parentOf(path), newName));
    toasts.add({ tone: "success", title: t("files.folderRenamed", { name: newName }) });
  };
  return (
    <ValueDialog
      title={t("files.renameFolder.title", { name })}
      label={t("files.rename.label")}
      submit={t("files.rename.submit")}
      initial={name}
      subjectOf={(typed) => typed.trim()}
      kind="folder"
      courseId={courseId}
      change={change}
      onClose={onClose}
    />
  );
}

function DeleteFile({ file, onClose }: { file: CourseFile; onClose: () => void }) {
  const { t } = useStepclimbStrings();
  const { toasts } = useAppContext();
  const mutation = useFilesChange(file.course_id, async () => {
    try {
      await deleteCourseFile(file.id);
    } catch (error) {
      const problem = filesProblem(error, file.name, t);
      toasts.add({
        tone: "error",
        title: t("files.delete.failed", { name: file.name }),
        description: problemSentence(problem, t),
      });
      throw error;
    }
    toasts.add({ tone: "success", title: t("files.deleted", { name: file.name }) });
  });
  return (
    <AlertDialog
      isOpen
      onOpenChange={(isOpen) => (isOpen ? null : onClose())}
      title={t("files.delete.title", { name: file.name })}
      confirmLabel={t("files.delete.confirm")}
      intent="danger"
      onConfirm={() => mutation.mutate(undefined)}
    >
      {t("files.delete.description")}
    </AlertDialog>
  );
}
