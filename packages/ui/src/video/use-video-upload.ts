// SPDX-License-Identifier: AGPL-3.0-or-later
// VideoUploader's upload, as state (docs/specs/m1/08-uploaded-video.md B22). One upload at a time:
// it is created (or found again in local storage and resumed from the offset `HEAD` answers),
// sent in chunks, paused, resumed and canceled; once complete, the video is read every 10 s until
// it is ready or failed. An upload stopped by a lost connection resumes by itself when the browser
// is back online. Unmounting aborts the chunk in flight and stops the reads; the upload stays in
// local storage, to resume after a reload; the entries are the owner's (D30).
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { fingerprint, isUploadAddress, type ResumeEntry, resumeStore } from "./resume-store";
import {
  type CreatedUpload,
  checkedLength,
  createUpload,
  endUpload,
  isAbort,
  readOffset,
  type Send,
  sendFile,
  TusProblem,
} from "./tus";
import { isResumable, type UploadProblem } from "./video-problems";

/** How often a processing video is read (B22). */
export const POLL_MS = 10_000;

/** A video as the uploader reads it: its status and, when it failed, why (B2). */
export interface VideoStatus {
  status: string;
  error_code?: string | null;
}

/**
 * Reads the video `id`; resolves `null` when it no longer exists. A rejection is a read that
 * failed, and the next one is tried 10 s later.
 */
export type GetVideo = (id: string, signal: AbortSignal) => Promise<VideoStatus | null>;

export type UploadPhase =
  | "starting"
  | "uploading"
  | "paused"
  | "interrupted"
  | "cancelling"
  | "processing"
  | "ready"
  | "failed"
  | "cancelled";

/** The upload on screen. */
export interface UploadState {
  phase: UploadPhase;
  name: string;
  size: number;
  /** Bytes the server holds, plus those of the chunk in flight. */
  sent: number;
  /** Bytes the server answered it holds: the last stored chunk's offset, or `HEAD`'s. */
  stored: number;
  /** Why the upload stopped or the video failed; also a refused cancel, beside `paused`. */
  problem?: UploadProblem;
  /** True when the reads of a processing video are failing; they go on. */
  pollFailing?: boolean;
  /** How the upload began: new, continued from local storage, or started again. */
  origin: "new" | "resumed" | "restarted";
}

export interface UseVideoUploadOptions {
  /** The creation address: `POST` starts an upload there. */
  endpoint: string;
  /** The course the uploads go to; local storage keeps each course's entries apart. */
  storageKey: string;
  /** Whose entries these are: the signed-in organization and user, such as `<org>.<user>` (D30). */
  owner: string;
  send: Send;
  getVideo: GetVideo;
  /** Called when a video is created, completes, settles or is deleted. */
  onChange?: () => void;
  /** Local storage; `globalThis.localStorage` by default. */
  storage?: Storage;
  /** Every chunk but the last; 16 MiB by default. */
  chunkBytes?: number;
}

/** Phases with a request of the uploader's own in flight. */
const ACTIVE: ReadonlySet<UploadPhase> = new Set(["starting", "uploading", "cancelling"]);

/** Phases from which the upload can be resumed or canceled. */
export const STOPPED: ReadonlySet<UploadPhase> = new Set(["paused", "interrupted"]);

function asProblem(error: unknown): UploadProblem {
  if (error instanceof TusProblem) {
    return { kind: "refused", status: error.status, code: error.code };
  }
  return { kind: "unreachable" };
}

function isGone(error: unknown): boolean {
  return error instanceof TusProblem && error.status === 404;
}

/** The upload is complete and its video is processing, so it can no longer be ended (D8). */
function isStatusInvalid(error: unknown): boolean {
  return error instanceof TusProblem && error.code === "video.status_invalid";
}

/** The address and video of the upload on screen, and its file while this page holds it. */
interface Current {
  file: File;
  url: string;
  videoId: string;
}

export function useVideoUpload(options: UseVideoUploadOptions) {
  const { endpoint, storageKey, owner, send, getVideo, chunkBytes } = options;
  const storage = options.storage ?? globalThis.localStorage;
  const store = useMemo(
    () => resumeStore(storage, owner, (url) => isUploadAddress(url, endpoint)),
    [storage, owner, endpoint],
  );
  const onChange = useRef(options.onChange);
  onChange.current = options.onChange;
  // The latest callbacks, so a caller's inline function does not restart the 10 s reads.
  const getVideoRef = useRef(getVideo);
  getVideoRef.current = getVideo;
  const [state, setState] = useState<UploadState>();
  const [pending, setPending] = useState<ResumeEntry[]>(() =>
    store.entries(storageKey, Date.now()),
  );
  const current = useRef<Current>(undefined);
  const controller = useRef<AbortController>(undefined);
  const phase = state?.phase;

  const refreshPending = useCallback(() => {
    const url = current.current?.url;
    setPending(store.entries(storageKey, Date.now()).filter((e) => e.url !== url));
  }, [store, storageKey]);

  /** A fresh controller for the next requests; aborting it stops them. */
  const nextSignal = () => {
    controller.current?.abort();
    const next = new AbortController();
    controller.current = next;
    return next.signal;
  };

  const update = (change: Partial<UploadState>) =>
    setState((previous) => (previous === undefined ? previous : { ...previous, ...change }));

  /** Records that the server holds `offset` bytes, on screen and in local storage. */
  const save = (offset: number) => {
    const upload = current.current;
    if (upload === undefined) return;
    update({ stored: offset });
    const { file, url, videoId } = upload;
    const entry = { scope: storageKey, fingerprint: fingerprint(file), url, videoId };
    store.save({ ...entry, name: file.name, size: file.size, offset, savedAt: Date.now() });
  };

  /** Stops showing the chunk in flight: what is on screen is what the server holds. */
  const settleSent = (change: Partial<UploadState>) =>
    setState((p) => (p === undefined ? p : { ...p, ...change, sent: p.stored }));

  /** Stops where a failure left it: resumable ones wait for Resume or the network. */
  const stop = (error: unknown) => {
    const problem = asProblem(error);
    const upload = current.current;
    if (isGone(error) && upload !== undefined) {
      store.remove(upload.url, Date.now());
      onChange.current?.();
    }
    settleSent({ phase: isResumable(problem) ? "interrupted" : "failed", problem });
  };

  /** Sends the file from `offset`, then reads the video until it settles. */
  const run = async (offset: number, signal: AbortSignal) => {
    const upload = current.current;
    if (upload === undefined) return;
    update({ phase: "uploading", sent: offset, stored: offset, problem: undefined });
    try {
      await sendFile(send, upload.url, upload.file, {
        offset,
        chunkBytes,
        signal,
        onProgress: (sent) => update({ sent }),
        onStored: save,
      });
    } catch (error) {
      if (!isAbort(error)) stop(error);
      return;
    }
    store.remove(upload.url, Date.now());
    update({ phase: "processing", sent: upload.file.size, stored: upload.file.size });
    onChange.current?.();
  };

  /** Resumes `upload` from the offset the server holds; a gone upload starts again. */
  const resumeStored = async (file: File, entry: ResumeEntry, signal: AbortSignal) => {
    try {
      const stored = checkedLength(await readOffset(send, entry.url, signal), file.size);
      current.current = { file, url: entry.url, videoId: entry.videoId };
      update({ origin: "resumed", sent: stored.offset, stored: stored.offset });
      refreshPending();
      await run(stored.offset, signal);
      return;
    } catch (error) {
      if (isAbort(error)) return;
      if (!isGone(error)) {
        current.current = { file, url: entry.url, videoId: entry.videoId };
        stop(error);
        return;
      }
    }
    store.remove(entry.url, Date.now());
    update({ origin: "restarted" });
    await create(file, signal);
  };

  const create = async (file: File, signal: AbortSignal) => {
    let created: CreatedUpload;
    try {
      created = await createUpload(send, endpoint, file, signal);
    } catch (error) {
      if (!isAbort(error)) update({ phase: "failed", problem: asProblem(error) });
      return;
    }
    current.current = { file, ...created };
    save(0);
    refreshPending();
    onChange.current?.();
    await run(0, signal);
  };

  /** Starts uploading `file`, or resumes this browser's earlier upload of it. False when busy. */
  const start = (file: File): boolean => {
    if (phase !== undefined && (ACTIVE.has(phase) || STOPPED.has(phase))) return false;
    current.current = undefined;
    const signal = nextSignal();
    setState({
      phase: "starting",
      name: file.name,
      size: file.size,
      sent: 0,
      stored: 0,
      origin: "new",
    });
    const entry = store.entryFor(storageKey, file, Date.now());
    void (entry === undefined ? create(file, signal) : resumeStored(file, entry, signal));
    return true;
  };

  const pause = () => {
    if (phase !== "uploading") return;
    controller.current?.abort();
    settleSent({ phase: "paused" });
  };

  const resume = () => {
    const upload = current.current;
    if (upload === undefined || phase === undefined || !STOPPED.has(phase)) return;
    const signal = nextSignal();
    update({ phase: "uploading", problem: undefined });
    void (async () => {
      try {
        const stored = checkedLength(await readOffset(send, upload.url, signal), upload.file.size);
        await run(stored.offset, signal);
      } catch (error) {
        if (!isAbort(error)) stop(error);
      }
    })();
  };

  const cancel = () => {
    const upload = current.current;
    if (upload === undefined || phase === undefined) return;
    if (phase !== "uploading" && !STOPPED.has(phase)) return;
    const signal = nextSignal();
    update({ phase: "cancelling", problem: undefined });
    void (async () => {
      try {
        await endUpload(send, upload.url, signal);
      } catch (error) {
        if (isAbort(error)) return;
        if (!isGone(error)) {
          const completed = isStatusInvalid(error);
          settleSent({ phase: completed ? "processing" : "paused", problem: asProblem(error) });
          if (completed) store.remove(upload.url, Date.now());
          return;
        }
      }
      store.remove(upload.url, Date.now());
      update({ phase: "cancelled" });
      onChange.current?.();
    })();
  };

  /**
   * Ends an unfinished upload this browser started before a reload. One the server no longer has,
   * or one already complete (409 `video.status_invalid`: another tab finished it), leaves the
   * list as well.
   */
  const discard = async (entry: ResumeEntry): Promise<UploadProblem | undefined> => {
    try {
      await endUpload(send, entry.url, new AbortController().signal);
    } catch (error) {
      if (!isGone(error) && !isStatusInvalid(error)) return asProblem(error);
    }
    store.remove(entry.url, Date.now());
    refreshPending();
    onChange.current?.();
    return undefined;
  };

  usePolling(phase === "processing", current, getVideoRef, setState, onChange);

  // Back online: an upload the network stopped goes on by itself.
  useEffect(() => {
    if (phase !== "interrupted") return;
    globalThis.addEventListener("online", resume);
    return () => globalThis.removeEventListener("online", resume);
  });

  useEffect(() => () => controller.current?.abort(), []);

  return { state, pending, start, pause, resume, cancel, discard };
}

/** While `active`, reads the current video every `POLL_MS` until it is ready, failed or gone. */
function usePolling(
  active: boolean,
  current: { current: Current | undefined },
  getVideo: { current: GetVideo },
  setState: (change: (previous: UploadState | undefined) => UploadState | undefined) => void,
  onChange: { current: (() => void) | undefined },
) {
  useEffect(() => {
    const videoId = current.current?.videoId;
    if (!active || videoId === undefined) return;
    const stopped = new AbortController();
    let timer: ReturnType<typeof setTimeout>;
    const settle = (change: Partial<UploadState>) => {
      setState((previous) => (previous === undefined ? previous : { ...previous, ...change }));
      onChange.current?.();
    };
    const read = async () => {
      let video: VideoStatus | null;
      try {
        video = await getVideo.current(videoId, stopped.signal);
      } catch {
        if (stopped.signal.aborted) return;
        setState((p) => (p === undefined ? p : { ...p, pollFailing: true }));
        timer = setTimeout(read, POLL_MS);
        return;
      }
      if (stopped.signal.aborted) return;
      if (video === null) settle({ phase: "failed", problem: { kind: "gone" } });
      else if (video.status === "ready") settle({ phase: "ready", pollFailing: false });
      else if (video.status === "failed") {
        const code = video.error_code ?? "video.failed";
        settle({ phase: "failed", problem: { kind: "video", code }, pollFailing: false });
      } else {
        setState((p) => (p === undefined ? p : { ...p, pollFailing: false }));
        timer = setTimeout(read, POLL_MS);
      }
    };
    timer = setTimeout(read, POLL_MS);
    return () => {
      stopped.abort();
      clearTimeout(timer);
    };
  }, [active, current, getVideo, setState, onChange]);
}
