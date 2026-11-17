// SPDX-License-Identifier: AGPL-3.0-or-later
// `/c/:course/pages/:page/edit`, the page authoring screen (docs/specs/m1/07-pages-and-the-editor.md
// B3, B4, B10, B21). It loads the page's current version, keeps the signed-in user's unsaved work
// in this browser with the version it was based on and offers it back, saves with Save or
// Ctrl/Cmd+S against the version the author started from, and on a conflict lets the author save
// their version over the newer one or load the newer one. Every refusal is the server's; the
// editor shows it at its block.
import {
  AlertDialog,
  Button,
  Callout,
  DialogTrigger,
  Skeleton,
  Text,
  useStepclimbStrings,
} from "@stepclimb/ui";
import { Editor, type EditorProblem, type PageDoc, sameDoc } from "@stepclimb/ui/editor";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useParams } from "@tanstack/react-router";
import { useCallback, useEffect, useId, useRef, useState } from "react";
import { ApiError, type Problem } from "../api";
import { useAppContext } from "../context";
import { LoadError } from "../layouts/load-error";
import {
  clearDraft,
  type DraftOwner,
  draftOwner,
  type PageDraft,
  readDraft,
  writeDraft,
} from "../pages/draft";
import {
  type EditablePage,
  editablePageQuery,
  savePageBody,
  uploadPageImage,
} from "../pages/page-api";
import { saveProblem, uploadMessage } from "../pages/problems";
import { meQuery } from "../session";

/** How long the author pauses before their work is written to local storage. */
const DRAFT_DELAY_MS = 1_000;

type CurrentVersion = NonNullable<Problem["current_version"]>;

export function PageEditView() {
  const { course, page } = useParams({ from: "/signed-in/c/$course/pages/$page/edit" });
  const query = useQuery(editablePageQuery(page));
  // The signed-in layout loaded `/me` before drawing this view; drafts are that user's.
  const me = useQuery(meQuery).data;
  const queryClient = useQueryClient();
  const { toasts } = useAppContext();
  const { t } = useStepclimbStrings();
  // Counts the times the author chose to load the newest version: each starts a new editing
  // session. A save does not, so the editor keeps its undo history and caret across saves.
  const [session, setSession] = useState(0);
  if (query.isPending || !me) {
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
  const owner = draftOwner(me);
  const reload = async () => {
    clearDraft(owner, page);
    await queryClient.refetchQueries({ queryKey: editablePageQuery(page).queryKey });
    const loaded = queryClient.getQueryData(editablePageQuery(page).queryKey);
    setSession((n) => n + 1);
    if (loaded) {
      toasts.add({
        tone: "success",
        title: t("pageEdit.loaded", { number: loaded.version.number }),
      });
    }
  };
  return (
    <PageEditor
      key={session}
      owner={owner}
      courseId={course}
      pageId={page}
      loaded={query.data}
      focusOnMount={session > 0}
      onReload={() => void reload()}
    />
  );
}

interface PageEditorProps {
  owner: DraftOwner;
  courseId: string;
  pageId: string;
  loaded: EditablePage;
  /** Put the caret in the page once it is drawn: after the author loaded the newest version. */
  focusOnMount: boolean;
  /** Drops the author's work and loads the page's newest version. */
  onReload: () => void;
}

/** The page's draft when it holds work the loaded body does not; a stale copy is dropped. */
function offeredDraft(owner: DraftOwner, pageId: string, loaded: EditablePage) {
  const draft = readDraft(owner, pageId);
  if (draft === undefined) return undefined;
  if (draft.base_version_id === loaded.version.id && sameDoc(draft.body, loaded.body)) {
    clearDraft(owner, pageId);
    return undefined;
  }
  return draft;
}

function PageEditor({ owner, courseId, pageId, loaded, focusOnMount, onReload }: PageEditorProps) {
  const { t } = useStepclimbStrings();
  const { toasts } = useAppContext();
  const queryClient = useQueryClient();
  const saveId = useId();
  const [body, setBody] = useState<PageDoc>(loaded.body);
  const [saved, setSaved] = useState<PageDoc>(loaded.body);
  const [base, setBase] = useState({ id: loaded.version.id, number: loaded.version.number });
  const [offer, setOffer] = useState(() => offeredDraft(owner, pageId, loaded));
  const [conflict, setConflict] = useState<CurrentVersion>();
  const [problem, setProblem] = useState<EditorProblem>();
  // By identity: every edit gives a new body and a save keeps the body it sent, so this costs
  // nothing per keystroke on a 1 MiB page. Undoing back to the saved text still counts as
  // unsaved; saving it then writes no version (B3).
  const dirty = body !== saved;

  const discardWork = useDraftWriter({ owner, pageId, base: base.id, body, dirty });

  const save = useMutation({
    mutationFn: (request: { body: PageDoc; base: string }) =>
      savePageBody(pageId, request.body, request.base),
    onMutate: () => {
      setProblem(undefined);
      setOffer(undefined);
    },
    onSuccess: (summary, request) => {
      const current = summary.current_version;
      const fromConflict = conflict !== undefined;
      setConflict(undefined);
      setSaved(request.body);
      setBase({ id: current.id, number: current.number });
      clearDraft(owner, pageId);
      queryClient.setQueryData(editablePageQuery(pageId).queryKey, {
        version: current,
        body: request.body,
      });
      toasts.add({ tone: "success", title: t("pageEdit.saved", { number: current.number }) });
      // The conflict notice and its button go with the conflict; Save is where to carry on.
      if (fromConflict) requestAnimationFrame(() => document.getElementById(saveId)?.focus());
    },
    onError: (error) => {
      const refused = error instanceof ApiError ? error.problem : undefined;
      if (refused?.code === "page.version_conflict" && refused.current_version) {
        setConflict(refused.current_version);
      } else {
        setConflict(undefined);
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

  const edit = (next: PageDoc) => {
    // The first edit answers the offer: the author carries on from what they see, and the
    // draft writer keeps their work from here.
    setOffer(undefined);
    setBody(next);
  };
  const restore = (draft: PageDraft) => {
    setBody(draft.body);
    if (draft.base_version_id !== base.id) setBase({ id: draft.base_version_id, number: 0 });
    setOffer(undefined);
  };

  return (
    <div className="flex flex-col gap-4 p-4 md:p-6">
      {offer ? (
        <DraftNotice
          draft={offer}
          onRestore={() => restore(offer)}
          onDiscard={() => {
            clearDraft(owner, pageId);
            setOffer(undefined);
          }}
        />
      ) : null}
      {conflict ? (
        <ConflictNotice
          current={conflict}
          isPending={save.isPending}
          onOverwrite={() => saveNow(conflict.id)}
          onReload={() => {
            discardWork();
            onReload();
          }}
        />
      ) : null}
      <Editor
        value={body}
        onChange={edit}
        problem={problem}
        autoFocus={focusOnMount}
        onUploadImage={async (file, onProgress) => {
          try {
            return await uploadPageImage(courseId, file, onProgress);
          } catch (error) {
            throw new Error(uploadMessage(error, t));
          }
        }}
        actions={
          <>
            {/* Not a live region: the toast announces a save once, and "Unsaved changes" would
                be read out on every first keystroke. */}
            <Text size="sm" tone="sub" className="me-auto">
              {saveState(t, save.isPending, dirty, base.number)}
            </Text>
            <Button
              id={saveId}
              intent="primary"
              size="sm"
              isPending={save.isPending}
              onPress={() => saveNow()}
            >
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

interface DraftWork {
  owner: DraftOwner;
  pageId: string;
  base: string;
  body: PageDoc;
  dirty: boolean;
}

/**
 * Writes the author's work to local storage once they pause while it differs from the saved
 * body, and at once when the screen goes away or the browser hides the page (a closed tab, a
 * phone switching apps), so the last second of typing is not lost.
 */
function useDraftWriter(work: DraftWork): () => void {
  const latest = useRef(work);
  latest.current = work;
  // Set when the author chose to drop their work, so leaving the screen does not write it back.
  const discarded = useRef(false);
  const { pageId, base, body, dirty } = work;
  const { orgId, userId } = work.owner;
  useEffect(() => {
    if (!dirty) return;
    const write = () => writeDraft({ orgId, userId }, pageId, base, body, Date.now());
    const timer = setTimeout(write, DRAFT_DELAY_MS);
    return () => clearTimeout(timer);
  }, [orgId, userId, pageId, base, body, dirty]);
  useEffect(() => {
    const flush = () => {
      const now = latest.current;
      if (now.dirty && !discarded.current) {
        writeDraft(now.owner, now.pageId, now.base, now.body, Date.now());
      }
    };
    window.addEventListener("pagehide", flush);
    return () => {
      window.removeEventListener("pagehide", flush);
      flush();
    };
  }, []);
  return () => {
    discarded.current = true;
  };
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
    <section aria-label={t("pageEdit.draft.label")}>
      <Callout kind="info">
        <div className="flex flex-col gap-3">
          <p>{t("pageEdit.draft.found", { when: new Date(draft.saved_at) })}</p>
          <div className="flex flex-wrap gap-2">
            <Button size="sm" intent="primary" onPress={onRestore}>
              {t("pageEdit.draft.restore")}
            </Button>
            <Button size="sm" onPress={onDiscard}>
              {t("pageEdit.draft.discard")}
            </Button>
          </div>
        </div>
      </Callout>
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
 * history, or to load the newer version and drop theirs. The notice stays while a save of theirs
 * is in flight, so the button they pressed keeps focus until the save settles.
 */
function ConflictNotice({ current, isPending, onOverwrite, onReload }: ConflictNoticeProps) {
  const { t } = useStepclimbStrings();
  return (
    <section aria-label={t("pageEdit.conflict.label")}>
      <Callout kind="warning" title={t("pageEdit.conflict.label")}>
        <div className="flex flex-col gap-3">
          <p role="alert">{t("pageEdit.conflict.message", { number: current.number })}</p>
          <div className="flex flex-wrap gap-2">
            <Button size="sm" intent="primary" isPending={isPending} onPress={onOverwrite}>
              {t("pageEdit.conflict.overwrite")}
            </Button>
            <DialogTrigger>
              <Button size="sm" isDisabled={isPending}>
                {t("pageEdit.conflict.reload", { number: current.number })}
              </Button>
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
        </div>
      </Callout>
    </section>
  );
}
