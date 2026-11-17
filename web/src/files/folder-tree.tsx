// SPDX-License-Identifier: AGPL-3.0-or-later
// The course's folders as a Tree (docs/specs/m1/16-learning-ui-components.md B7, B20). The top
// folder is the one root row. A folder's sub-folders are asked for when it is opened, so a course
// with many folders loads only the ones a person looks into; until then the row offers to open.
// Selecting a row opens that folder in the list beside it. The folder on screen and every folder
// above it start open. When a folder's sub-folders could not be loaded, a line under the tree
// says so and offers "Try again".
import { FolderIcon, FolderOpenIcon, WarningCircleIcon } from "@phosphor-icons/react";
import { Button, Icon, Text, Tree, TreeItem, useStepclimbStrings } from "@stepclimb/ui";
import { type UseQueryResult, useQueries } from "@tanstack/react-query";
import { useState } from "react";
import type { Key } from "react-aria-components";
import { subfoldersQuery } from "./course-files";
import { folderName, foldersDownTo, TOP_FOLDER } from "./folders";

export interface FolderTreeProps {
  courseId: string;
  /** The folder on screen: the selected row. */
  folder: string;
  onOpenFolder: (folder: string) => void;
  /** Layout only. */
  className?: string;
}

interface Loaded {
  /** Each open folder's sub-folders, once they have loaded. */
  children: Map<string, string[]>;
  /** The queries of open folders whose sub-folders could not be loaded. */
  failed: UseQueryResult<string[]>[];
}

/** The open folders' sub-folders, asked for as the folders open. */
function useSubfolders(courseId: string, open: string[]): Loaded {
  return useQueries({
    queries: open.map((path) => subfoldersQuery(courseId, path)),
    combine: (results) => ({
      children: new Map(
        open.flatMap((path, i) => {
          const data = results[i]?.data;
          return data === undefined ? [] : [[path, data] as const];
        }),
      ),
      failed: results.filter((result) => result.isError),
    }),
  });
}

/** Which rows are open: the folder on screen and those above it open whenever it changes. */
function useExpanded(folder: string) {
  const [expanded, setExpanded] = useState<Set<Key>>(() => new Set(foldersDownTo(folder)));
  const [openedFor, setOpenedFor] = useState(folder);
  if (openedFor !== folder) {
    setOpenedFor(folder);
    setExpanded((keys) => new Set([...keys, ...foldersDownTo(folder)]));
  }
  return [expanded, setExpanded] as const;
}

export function FolderTree({ courseId, folder, onOpenFolder, className }: FolderTreeProps) {
  const { t } = useStepclimbStrings();
  const [expanded, setExpanded] = useExpanded(folder);
  const open = [...expanded].filter((key): key is string => typeof key === "string");
  const { children: loaded, failed } = useSubfolders(courseId, open);

  function row(path: string) {
    const children = loaded.get(path);
    const shown = expanded.has(path) && children !== undefined;
    const title = path === TOP_FOLDER ? t("files.topFolder") : (folderName(path) ?? path);
    return (
      <TreeItem
        key={path}
        id={path}
        title={title}
        icon={shown ? FolderOpenIcon : FolderIcon}
        hasChildItems={children === undefined || children.length > 0}
      >
        {shown ? children.map(row) : null}
      </TreeItem>
    );
  }

  return (
    <div className={`flex flex-col gap-2 ${className ?? ""}`}>
      <Tree
        aria-label={t("files.folders")}
        selectionMode="single"
        disallowEmptySelection
        selectedKeys={[folder]}
        onSelectionChange={(keys) => {
          const [key] = keys === "all" ? [] : [...keys];
          if (typeof key === "string" && key !== folder) onOpenFolder(key);
        }}
        expandedKeys={expanded}
        onExpandedChange={setExpanded}
      >
        {row(TOP_FOLDER)}
      </Tree>
      {failed.length === 0 ? null : (
        <div data-slot="files-tree-error" className="flex flex-wrap items-center gap-x-3 gap-y-2">
          <Text size="sm" className="flex items-start gap-1.5">
            <Icon icon={WarningCircleIcon} size="sm" className="mt-[0.15em] shrink-0" />
            {t("files.tree.failed")}
          </Text>
          <Button
            size="sm"
            onPress={() => {
              for (const query of failed) void query.refetch();
            }}
          >
            {t("loadError.retry")}
          </Button>
        </div>
      )}
    </div>
  );
}
