// SPDX-License-Identifier: AGPL-3.0-or-later
// The course's folders as a Tree (docs/specs/m1/16-learning-ui-components.md B7, B20). The top
// folder is the one root row. A folder's sub-folders are asked for when it is opened, so a course
// with many folders loads only the ones a person looks into; until then the row offers to open.
// Selecting a row opens that folder in the list beside it. The folder on screen and every folder
// above it start open.
import { FolderIcon, FolderOpenIcon } from "@phosphor-icons/react";
import { Tree, TreeItem, useStepclimbStrings } from "@stepclimb/ui";
import { useQueries } from "@tanstack/react-query";
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

export function FolderTree({ courseId, folder, onOpenFolder, className }: FolderTreeProps) {
  const { t } = useStepclimbStrings();
  const [expanded, setExpanded] = useState<Set<Key>>(() => new Set(foldersDownTo(folder)));
  // A folder opened from the list or the breadcrumbs opens its rows in the tree too.
  const [openedFor, setOpenedFor] = useState(folder);
  if (openedFor !== folder) {
    setOpenedFor(folder);
    setExpanded((keys) => new Set([...keys, ...foldersDownTo(folder)]));
  }
  const open = [...expanded].filter((key): key is string => typeof key === "string");
  const loaded = useQueries({
    queries: open.map((path) => subfoldersQuery(courseId, path)),
    combine: (results) =>
      new Map(
        open.flatMap((path, i) => {
          const data = results[i]?.data;
          return data === undefined ? [] : [[path, data] as const];
        }),
      ),
  });

  function row(path: string) {
    const children = loaded.get(path);
    const isOpen = expanded.has(path);
    const title = path === TOP_FOLDER ? t("files.topFolder") : (folderName(path) ?? path);
    return (
      <TreeItem
        key={path}
        id={path}
        title={title}
        icon={isOpen && children !== undefined ? FolderOpenIcon : FolderIcon}
        hasChildItems={children === undefined || children.length > 0}
      >
        {isOpen && children !== undefined ? children.map(row) : null}
      </TreeItem>
    );
  }

  return (
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
      className={className}
    >
      {row(TOP_FOLDER)}
    </Tree>
  );
}
