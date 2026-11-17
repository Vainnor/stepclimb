// SPDX-License-Identifier: AGPL-3.0-or-later
// `/c/:course/pages/:page/edit`, the page authoring screen (docs/specs/m1/07-pages-and-the-editor.md
// B3, B4, B10, B21). It loads the page's current version, keeps unsaved work in this browser keyed
// by page and base version and offers it back, saves with Save or Ctrl/Cmd+S against the version
// the author started from, and on a conflict lets the author save their version over the newer
// one or load the newer one. Every refusal is the server's; the editor shows it at its block.
import {
  AlertDialog,
  Button,
  DialogTrigger,
  FormError,
  Skeleton,
  Text,
  useStepclimbStrings,
} from "@stepclimb/ui";
import { Editor, type EditorProblem, type PageDoc, sameDoc } from "@stepclimb/ui/editor";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useParams } from "@tanstack/react-router";
import { useCallback, useEffect, useRef, useState } from "react";
import { ApiError, type Problem } from "../api";
import { useAppContext } from "../context";
import { LoadError } from "../layouts/load-error";
import { clearDraft, type PageDraft, readDraft, writeDraft } from "../pages/draft";
import {
  type EditablePage,
  editablePageQuery,
  savePageBody,
  uploadPageImage,
} from "../pages/page-api";
import { saveProblem, uploadMessage } from "../pages/problems";

/** How long the author pauses before their work is written to local storage. */
const DRAFT_DELAY_MS = 1_000;

type CurrentVersion = NonNullable<Problem["current_version"]>;

export function PageEditView() {
  const { course, page } = useParams({ from: "/signed-in/c/$course/pages/$page/edit" });
  const query = useQuery(editablePageQuery(page));
  if (query.isPending) {
    return (
      <div aria-busy="true" className="flex flex-col gap-3 p-4 md:p-6">
        <Skeleton className="h-12 w-full" />
        <Skeleton className="h-64 w-full" />
      </div>
    );
  }
  if (query.isError) {
    return (
      <div className="p-4 md:p-6">
        <LoadError error={query.error} level={2} onRetry={() => void query.refetch()} />
      </div>
    );
  }
  // A newly loaded version starts a new editing session.
  return (
    <PageEditor key={query.data.version.id} courseId={course} pageId={page} loaded={query.data} />
  );
}

interface PageEditorProps {
  courseId: string;
  pageId: string;
  loaded: EditablePage;
}

/** The page's draft when it holds work the loaded body does not; a stale copy is dropped. */
function offeredDraft(pageId: string, loaded: EditablePage): PageDraft | undefined {
  const draft = readDraft(pageId);
  if (draft === undefined) return undefined;
  if (draft.base_version_id === loaded.version.id && sameDoc(draft.body, loaded.body)) {
    clearDraft(pageId);
    return undefined;
  }
  return draft;
}

function PageEditor({ courseId, pageId, loaded }: PageEditorProps) {
  const { t } = useStepclimbStrings();
  const { toasts } = useAppContext();
  const queryClient = useQueryClient();
  const [body, setBody] = useState<PageDoc>(loaded.body);
  const [saved, setSaved] = useState<PageDoc>(loaded.body);
  const [base, setBase] = useState({ id: loaded.version.id, number: loaded.version.number });
  const [draft, setDraft] = useState(() => offeredDraft(pageId, loaded));
  const [conflict, setConflict] = useState<CurrentVersion>();
  const [problem, setProblem] = useState<EditorProblem>();
  const dirty = !sameDoc(body, saved);

  useDraftWriter(pageId, base.id, body, dirty && draft === undefined);

  const save = useMutation({
    mutationFn: (request: { body: PageDoc; base: string }) =>
      savePageBody(pageId, request.body, request.base),
    onMutate: () => {
      setProblem(undefined);
      setConflict(undefined);
    },
    onSuccess: (summary, request) => {
      const current = summary.current_version;
      setSaved(request.body);
      setBase({ id: current.id, number: current.number });
      clearDraft(pageId);
      queryClient.setQueryData(editablePageQuery(pageId).queryKey, {
        version: current,
        body: request.body,
      });
      toasts.add({ tone: "success", title: t("pageEdit.saved", { number: current.number }) });
    },
    onError: (error) => {
      const refused = error instanceof ApiError ? error.problem : undefined;
      if (refused?.code === "page.version_conflict" && refused.current_version) {
        setConflict(refused.current_version);
      } else {
        setProblem(saveProblem(error, t));
      }
    },
  });

  const saveNow = useCallback(
    (over?: string) => {
      if (save.isPending) return;
      save.mutate({ body, base: over ?? base.id });
    },
    [save, body, base.id],
  );
  useSaveShortcut(saveNow);

  const reload = () => {
    clearDraft(pageId);
    void queryClient.invalidateQueries({ queryKey: editablePageQuery(pageId).queryKey });
  };
  const restore = (offered: PageDraft) => {
    setBody(offered.body);
    if (offered.base_version_id !== base.id) setBase({ id: offered.base_version_id, number: 0 });
    setDraft(undefined);
  };

  return (
    <div className="flex flex-col gap-4 p-4 md:p-6">
      {draft ? (
        <DraftNotice
          draft={draft}
          onRestore={() => restore(draft)}
          onDiscard={() => {
            clearDraft(pageId);
            setDraft(undefined);
          }}
        />
      ) : null}
      {conflict ? (
        <ConflictNotice
          current={conflict}
          isPending={save.isPending}
          onOverwrite={() => saveNow(conflict.id)}
          onReload={reload}
        />
      ) : null}
      <Editor
        value={body}
        onChange={setBody}
        problem={problem}
        onUploadImage={async (file, onProgress) => {
          try {
            return await uploadPageImage(courseId, file, onProgress);
          } catch (error) {
            throw new Error(uploadMessage(error, t));
          }
        }}
        actions={
          <>
            <Text size="sm" tone="sub" aria-live="polite" className="me-auto">
              {saveState(t, save.isPending, dirty, base.number)}
            </Text>
            <Button intent="primary" size="sm" isPending={save.isPending} onPress={() => saveNow()}>
              {t("pageEdit.save")}
            </Button>
          </>
        }
      />
    </div>
  );
}

function saveState(
  t: ReturnType<typeof useStepclimbStrings>["t"],
  saving: boolean,
  dirty: boolean,
  number: number,
): string {
  if (saving) return t("pageEdit.state.saving");
  if (dirty) return t("pageEdit.state.unsaved");
  return number > 0 ? t("pageEdit.state.saved", { number }) : t("pageEdit.state.unsaved");
}

/** Writes the author's work to local storage once they pause, while it differs from the saved. */
function useDraftWriter(pageId: string, base: string, body: PageDoc, active: boolean) {
  useEffect(() => {
    if (!active) return;
    const timer = setTimeout(() => writeDraft(pageId, base, body, Date.now()), DRAFT_DELAY_MS);
    return () => clearTimeout(timer);
  }, [pageId, base, body, active]);
}

/** Ctrl+S, or Cmd+S on a Mac, saves instead of the browser saving the HTML file (B4). */
function useSaveShortcut(save: () => void) {
  const current = useRef(save);
  current.current = save;
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && !event.altKey && event.key.toLowerCase() === "s") {
        event.preventDefault();
        current.current();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
}

interface DraftNoticeProps {
  draft: PageDraft;
  onRestore: () => void;
  onDiscard: () => void;
}

/** Unsaved work from an earlier visit, offered back (B4). */
function DraftNotice({ draft, onRestore, onDiscard }: DraftNoticeProps) {
  const { t } = useStepclimbStrings();
  return (
    <section
      aria-label={t("pageEdit.draft.label")}
      className="flex flex-wrap items-center gap-3 rounded-md border border-line bg-raised p-3"
    >
      <Text elementType="p" className="min-w-0 flex-1">
        {t("pageEdit.draft.found", { when: new Date(draft.saved_at) })}
      </Text>
      <div className="flex gap-2">
        <Button size="sm" intent="primary" onPress={onRestore}>
          {t("pageEdit.draft.restore")}
        </Button>
        <Button size="sm" onPress={onDiscard}>
          {t("pageEdit.draft.discard")}
        </Button>
      </div>
    </section>
  );
}

interface ConflictNoticeProps {
  current: CurrentVersion;
  isPending: boolean;
  onOverwrite: () => void;
  onReload: () => void;
}

/**
 * Someone saved a newer version while the author was editing (B3). Their work stays in the editor
 * and in local storage; they choose to save it over the newer version, which stays in the
 * history, or to load the newer version and drop theirs.
 */
function ConflictNotice({ current, isPending, onOverwrite, onReload }: ConflictNoticeProps) {
  const { t } = useStepclimbStrings();
  return (
    <section
      aria-label={t("pageEdit.conflict.label")}
      className="flex flex-col gap-3 rounded-md border border-line bg-raised p-3"
    >
      <FormError>{t("pageEdit.conflict.message", { number: current.number })}</FormError>
      <div className="flex flex-wrap gap-2">
        <Button size="sm" intent="primary" isPending={isPending} onPress={onOverwrite}>
          {t("pageEdit.conflict.overwrite")}
        </Button>
        <DialogTrigger>
          <Button size="sm">{t("pageEdit.conflict.reload", { number: current.number })}</Button>
          <AlertDialog
            title={t("pageEdit.conflict.reloadTitle", { number: current.number })}
            confirmLabel={t("pageEdit.conflict.reloadConfirm")}
            intent="danger"
            onConfirm={onReload}
          >
            {t("pageEdit.conflict.reloadDescription", { number: current.number })}
          </AlertDialog>
        </DialogTrigger>
      </div>
    </section>
  );
}
