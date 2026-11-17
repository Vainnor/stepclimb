// SPDX-License-Identifier: AGPL-3.0-or-later
// One folder's entries as a DataTable (docs/specs/m1/06-files.md B12;
// 16-learning-ui-components.md B20): sub-folders first, then files, each by name, in the server's
// order and a page at a time. A sub-folder's name opens it. Each file shows its size and, under its
// name, the type the server read from its bytes and when it last changed, which keeps the table to
// three columns that fit a 375 px screen. The type is a short label (B20a); the full MIME type is
// in the label's tooltip text and read to screen readers after it. The actions column is there
// only for a person who may change the course's content. When a dialog from a row's actions
// closes and the row then leaves the list (deleted, moved out, a folder renamed), focus goes to the
// row now in its place, or to the table when the folder is empty, never to the page's body.
import { FileIcon, FolderIcon, FolderOpenIcon } from "@phosphor-icons/react";
import {
  DataTable,
  type DataTableColumn,
  EmptyState,
  Icon,
  Link,
  type PaginationProps,
  type Strings,
  Text,
  useStepclimbStrings,
  VisuallyHidden,
} from "@stepclimb/ui";
import { useEffect, useMemo, useRef } from "react";
import type { CourseFile, FolderEntry } from "./course-files";
import { FileActions, FolderActions } from "./file-actions";
import { fileTypeLabel } from "./file-type";
import { TOP_FOLDER } from "./folders";

/** A table row: a folder entry with the key DataTable needs. */
type EntryRow = FolderEntry & { id: string };

function rowOf(entry: FolderEntry): EntryRow {
  return entry.kind === "folder"
    ? { ...entry, id: `folder:${entry.path}` }
    : { ...entry, id: entry.file.id };
}

/** A byte count in words: bytes under a kilobyte, then decimal kilobytes, megabytes, gigabytes. */
export function fileSize(bytes: number, t: Strings["t"]): string {
  if (bytes < 1e3) return t("files.size.bytes", { count: bytes });
  if (bytes < 1e6) return t("fileDrop.size", { unit: "kB", value: bytes / 1e3 });
  if (bytes < 1e9) return t("fileDrop.size", { unit: "MB", value: bytes / 1e6 });
  return t("fileDrop.size", { unit: "GB", value: bytes / 1e9 });
}

export interface FileTableProps {
  courseId: string;
  folder: string;
  entries: readonly FolderEntry[];
  isLoading: boolean;
  /** Whether rows get the rename, move and delete actions. */
  canEdit: boolean;
  /** The `href` of the files area at a folder. */
  folderHref: (folder: string) => string;
  pagination?: Omit<PaginationProps, "className">;
  /** The table's name. */
  label: string;
}

function NameCell({ row, folderHref }: { row: EntryRow; folderHref: (folder: string) => string }) {
  if (row.kind === "folder") {
    return (
      <span className="flex min-w-40 items-center gap-2">
        <Icon icon={FolderIcon} size="sm" className="shrink-0" />
        <Link
          href={folderHref(row.path)}
          // On a touch screen the link fills the row's height: a 44 px target (ui.md).
          className="wrap-anywhere pointer-coarse:inline-flex pointer-coarse:min-h-(--sc-control-touch) pointer-coarse:min-w-(--sc-control-touch) pointer-coarse:items-center"
        >
          {row.name}
        </Link>
      </span>
    );
  }
  return (
    <span className="flex min-w-40 items-center gap-2">
      <Icon icon={FileIcon} size="sm" className="shrink-0" />
      <span className="flex min-w-0 flex-col">
        <Text tone="strong" className="wrap-anywhere">
          {row.file.name}
        </Text>
        <FileDetails file={row.file} />
      </span>
    </span>
  );
}

/** A file's type, as the server read it, and when it last changed: the line under its name. */
function FileDetails({ file }: { file: CourseFile }) {
  const { t } = useStepclimbStrings();
  return (
    <Text size="sm" tone="sub" className="wrap-anywhere">
      <span title={file.mime}>
        {t("files.details", {
          type: fileTypeLabel(file.mime, file.name, t),
          updated: new Date(file.updated_at),
        })}
      </span>
      <VisuallyHidden>{t("files.type.mime", { mime: file.mime })}</VisuallyHidden>
    </Text>
  );
}

function useColumns(
  { courseId, canEdit, folderHref }: FileTableProps,
  onClosed: (rowId: string) => void,
) {
  const { t } = useStepclimbStrings();
  const file = (row: EntryRow): CourseFile | undefined =>
    row.kind === "file" ? row.file : undefined;
  const columns: DataTableColumn<EntryRow>[] = [
    {
      id: "name",
      label: t("files.column.name"),
      render: (row) => <NameCell row={row} folderHref={folderHref} />,
    },
    {
      id: "size",
      label: t("files.column.size"),
      align: "end",
      render: (row) => {
        const f = file(row);
        return f === undefined ? null : (
          <span className="whitespace-nowrap">{fileSize(f.size, t)}</span>
        );
      },
    },
  ];
  if (canEdit) {
    columns.push({
      id: "actions",
      label: t("files.column.actions"),
      align: "end",
      render: (row) =>
        row.kind === "folder" ? (
          <FolderActions
            courseId={courseId}
            path={row.path}
            name={row.name}
            onClosed={() => onClosed(row.id)}
          />
        ) : (
          <FileActions file={row.file} onClosed={() => onClosed(row.id)} />
        ),
    });
  }
  return columns;
}

/**
 * Keeps focus in the table when the row whose dialog last closed leaves the list: it goes to the
 * row now at that place, the last row, or the table itself when the folder is empty. A row that
 * is still listed, or focus already elsewhere, leaves focus where it is.
 */
function useFocusAfterRemoval(rows: readonly EntryRow[]) {
  const box = useRef<HTMLDivElement>(null);
  const closed = useRef<{ id: string; index: number }>(undefined);
  useEffect(() => {
    const row = closed.current;
    if (row === undefined || rows.some((r) => r.id === row.id)) return;
    closed.current = undefined;
    const active = document.activeElement;
    if (active !== null && active !== document.body && active.isConnected) return;
    const grid = box.current?.querySelector<HTMLElement>('[role="grid"]');
    const listed = [...(grid?.querySelectorAll<HTMLElement>("tbody > [role=row]") ?? [])];
    const target = rows.length === 0 ? grid : listed[Math.min(row.index, listed.length - 1)];
    target?.focus();
  }, [rows]);
  const onClosed = (id: string) => {
    closed.current = { id, index: rows.findIndex((r) => r.id === id) };
  };
  return { box, onClosed };
}

export function FileTable(props: FileTableProps) {
  const { t } = useStepclimbStrings();
  const rows = useMemo(() => props.entries.map(rowOf), [props.entries]);
  const focus = useFocusAfterRemoval(rows);
  const columns = useColumns(props, focus.onClosed);
  const empty =
    props.folder === TOP_FOLDER
      ? props.canEdit
        ? t("files.empty.courseEdit")
        : t("files.empty.course")
      : t("files.empty.folder");
  return (
    <div ref={focus.box} className="contents">
      <DataTable
        aria-label={props.label}
        columns={columns}
        rows={rows}
        isLoading={props.isLoading}
        emptyState={
          <EmptyState icon={props.folder === TOP_FOLDER ? FileIcon : FolderOpenIcon}>
            {empty}
          </EmptyState>
        }
        pagination={props.pagination}
      />
    </div>
  );
}
