// SPDX-License-Identifier: AGPL-3.0-or-later
// The details an image or a video embed needs before it is inserted
// (docs/specs/m1/07-pages-and-the-editor.md B10, B10a, B12), asked in a panel inside the editor
// rather than a modal: an image's file and text alternative, an embed's address and title.
import { type FormEvent, useEffect, useId, useRef, useState } from "react";
import { tv } from "../lib/tv";
import { FileDrop } from "../patterns/file-drop";
import { Button } from "../primitives/button";
import { Checkbox } from "../primitives/checkbox";
import { Heading } from "../primitives/heading";
import { TextField } from "../primitives/text-field";
import { useStepclimbStrings } from "../strings/strings";
import { imageTypes, MAX_EMBED_TITLE_CHARS } from "./page-doc";
import { videoFromUrl } from "./video-url";

export const insertPanel = tv({
  slots: {
    root: "flex flex-col gap-3 border-t border-line-soft bg-sunken p-3",
    fields: "flex flex-col gap-3 sm:max-w-md",
    actions: "flex flex-wrap gap-2",
  },
});

const styles = insertPanel();

/** A course file uploaded for an image: its id and the type the server sniffed. */
export interface UploadedImage {
  course_file_id: string;
  mime: string;
}

interface PanelProps {
  onInsert: (attrs: Record<string, unknown>) => void;
  onCancel: () => void;
}

export interface ImagePanelProps extends PanelProps {
  /** Uploads the file as a course file, reporting the share sent so far, from 0 to 1. */
  onUpload: (file: File, onProgress: (fraction: number) => void) => Promise<UploadedImage>;
}

/** Why an upload failed, as the message to show. */
function uploadProblem(error: unknown, t: (id: string) => string): string {
  return error instanceof Error && error.message !== ""
    ? error.message
    : t("editor.image.uploadFailed");
}

export function ImagePanel({ onUpload, onInsert, onCancel }: ImagePanelProps) {
  const { t } = useStepclimbStrings();
  const headingId = useId();
  const form = useRef<HTMLFormElement>(null);
  // The panel opens from a menu that has closed: focus goes to its first control, "Choose file".
  // A frame later, after the menu's own focus restore has run.
  useEffect(() => {
    const frame = requestAnimationFrame(() =>
      form.current?.querySelector<HTMLElement>('[data-slot="file-drop-choose"]')?.focus(),
    );
    return () => cancelAnimationFrame(frame);
  }, []);
  const [uploading, setUploading] = useState<{ name: string; fraction: number }>();
  const [file, setFile] = useState<UploadedImage>();
  const [problem, setProblem] = useState<string>();
  const [alt, setAlt] = useState("");
  const [decorative, setDecorative] = useState(false);
  const [tried, setTried] = useState(false);

  const select = async ([chosen]: File[]) => {
    if (!chosen) return;
    setProblem(undefined);
    setFile(undefined);
    setUploading({ name: chosen.name, fraction: 0 });
    try {
      const uploaded = await onUpload(chosen, (fraction) =>
        setUploading({ name: chosen.name, fraction }),
      );
      // The server sniffed the bytes; only these types may be page images (B10a).
      if ((imageTypes as readonly string[]).includes(uploaded.mime)) setFile(uploaded);
      else setProblem(t("editor.image.notAnImage"));
    } catch (error) {
      setProblem(uploadProblem(error, t));
    } finally {
      setUploading(undefined);
    }
  };
  const submit = (event: FormEvent) => {
    event.preventDefault();
    setTried(true);
    if (!file || (!decorative && alt.trim() === "")) return;
    onInsert({ course_file_id: file.course_file_id, alt: decorative ? "" : alt, decorative });
  };

  return (
    <form
      ref={form}
      data-slot="insert-panel"
      className={styles.root()}
      onSubmit={submit}
      aria-labelledby={headingId}
    >
      <Heading level={3} size="base" id={headingId}>
        {t("editor.image.title")}
      </Heading>
      <div data-slot="insert-panel-fields" className={styles.fields()}>
        <FileDrop
          aria-label={t("editor.image.file")}
          acceptedFileTypes={imageTypes}
          description={t("editor.image.types")}
          onSelect={(files) => void select(files)}
          upload={uploading}
          errorMessage={problem ?? (tried && !file ? t("editor.image.fileMissing") : undefined)}
        />
        <TextField
          validationBehavior="aria"
          label={t("editor.image.alt")}
          description={t("editor.image.altDescription")}
          value={alt}
          onChange={setAlt}
          isDisabled={decorative}
          isRequired={!decorative}
          isInvalid={tried && !decorative && alt.trim() === ""}
          errorMessage={t("editor.image.altMissing")}
        />
        <Checkbox isSelected={decorative} onChange={setDecorative}>
          {t("editor.image.decorativeOption")}
        </Checkbox>
      </div>
      <div data-slot="insert-panel-actions" className={styles.actions()}>
        <Button type="submit" intent="primary" size="sm" isDisabled={uploading !== undefined}>
          {t("editor.insertBlock", { kind: "image" })}
        </Button>
        <Button size="sm" onPress={onCancel}>
          {t("editor.cancel")}
        </Button>
      </div>
    </form>
  );
}

export function VideoPanel({ onInsert, onCancel }: PanelProps) {
  const { t } = useStepclimbStrings();
  const headingId = useId();
  const [address, setAddress] = useState("");
  const [title, setTitle] = useState("");
  const [tried, setTried] = useState(false);
  const video = videoFromUrl(address);

  const submit = (event: FormEvent) => {
    event.preventDefault();
    setTried(true);
    if (!video || title.trim() === "") return;
    onInsert({ ...video, title: title.trim() });
  };

  return (
    <form
      data-slot="insert-panel"
      className={styles.root()}
      onSubmit={submit}
      aria-labelledby={headingId}
    >
      <Heading level={3} size="base" id={headingId}>
        {t("editor.video.title")}
      </Heading>
      <div data-slot="insert-panel-fields" className={styles.fields()}>
        <TextField
          validationBehavior="aria"
          label={t("editor.video.address")}
          description={t("editor.video.addressDescription")}
          type="url"
          inputMode="url"
          autoFocus
          isRequired
          value={address}
          onChange={setAddress}
          isInvalid={tried && video === null}
          errorMessage={t("editor.video.addressInvalid")}
        />
        <TextField
          validationBehavior="aria"
          label={t("editor.label", { field: "title" })}
          description={t("editor.video.titleDescription")}
          isRequired
          maxLength={MAX_EMBED_TITLE_CHARS}
          value={title}
          onChange={setTitle}
          isInvalid={tried && title.trim() === ""}
          errorMessage={t("editor.labelMissing", { field: "title" })}
        />
      </div>
      <div data-slot="insert-panel-actions" className={styles.actions()}>
        <Button type="submit" intent="primary" size="sm">
          {t("editor.insertBlock", { kind: "videoEmbed" })}
        </Button>
        <Button size="sm" onPress={onCancel}>
          {t("editor.cancel")}
        </Button>
      </div>
    </form>
  );
}
