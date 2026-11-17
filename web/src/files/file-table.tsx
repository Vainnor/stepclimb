// SPDX-License-Identifier: AGPL-3.0-or-later
// One folder's entries as a DataTable (docs/specs/m1/06-files.md B12;
// 16-learning-ui-components.md B20): sub-folders first, then files, each by name, in the server's
// order and a page at a time. A sub-folder's name opens it. Each file shows its size, the type the
// server read from its bytes and when it last changed. The actions column is there only for a
// person who may change the course's content.
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
} from "@stepclimb/ui";
import type { CourseFile, FolderEntry } from "./course-files";
import { FileActions, FolderActions } from "./file-actions";
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
      <span className="flex min-w-0 items-center gap-2">
        <Icon icon={FolderIcon} size="sm" className="shrink-0" />
        <Link href={folderHref(row.path)} className="wrap-anywhere">
          {row.name}
        </Link>
      </span>
    );
  }
  return (
    <span className="flex min-w-0 items-center gap-2">
      <Icon icon={FileIcon} size="sm" className="shrink-0" />
      <Text tone="strong" className="wrap-anywhere">
        {row.file.name}
      </Text>
    </span>
  );
}

function useColumns({ courseId, canEdit, folderHref }: FileTableProps) {
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
        return f === undefined ? null : fileSize(f.size, t);
      },
    },
    { id: "type", label: t("files.column.type"), render: (row) => file(row)?.mime ?? null },
    {
      id: "updated",
      label: t("files.column.updated"),
      render: (row) => {
        const f = file(row);
        return f === undefined ? null : t("files.updated", { updated: new Date(f.updated_at) });
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
          <FolderActions courseId={courseId} path={row.path} name={row.name} />
        ) : (
          <FileActions file={row.file} />
        ),
    });
  }
  return columns;
}

export function FileTable(props: FileTableProps) {
  const { t } = useStepclimbStrings();
  const columns = useColumns(props);
  const empty =
    props.folder === TOP_FOLDER
      ? props.canEdit
        ? t("files.empty.courseEdit")
        : t("files.empty.course")
      : t("files.empty.folder");
  return (
    <DataTable
      aria-label={props.label}
      columns={columns}
      rows={props.entries.map(rowOf)}
      isLoading={props.isLoading}
      emptyState={
        <EmptyState icon={props.folder === TOP_FOLDER ? FileIcon : FolderOpenIcon}>
          {empty}
        </EmptyState>
      }
      pagination={props.pagination}
    />
  );
}
