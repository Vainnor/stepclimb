// SPDX-License-Identifier: AGPL-3.0-or-later
// A YouTube or Vimeo address to the provider and video id a page stores
// (docs/specs/m1/07-pages-and-the-editor.md B12): authors paste a URL, the editor keeps only the
// id, and the server builds the player's URL from it.

export type VideoProvider = "youtube" | "vimeo";

export interface VideoRef {
  provider: VideoProvider;
  video_id: string;
}

/** The id formats the server accepts (`VideoProvider::id_is_valid`). */
export function videoIdIsValid(provider: VideoProvider, id: string): boolean {
  return provider === "youtube" ? /^[A-Za-z0-9_-]{11}$/.test(id) : /^[0-9]{1,20}$/.test(id);
}

const YOUTUBE_HOSTS = new Set([
  "youtube.com",
  "www.youtube.com",
  "m.youtube.com",
  "youtube-nocookie.com",
  "www.youtube-nocookie.com",
]);
const VIMEO_HOSTS = new Set(["vimeo.com", "www.vimeo.com", "player.vimeo.com"]);

/** The YouTube id in `url`: `watch?v=<id>`, `/embed/<id>`, `/shorts/<id>`, `/live/<id>`. */
function youtubeId(url: URL): string | undefined {
  if (url.hostname === "youtu.be") return url.pathname.split("/")[1];
  if (!YOUTUBE_HOSTS.has(url.hostname)) return undefined;
  if (url.pathname === "/watch") return url.searchParams.get("v") ?? undefined;
  const [, kind, id] = url.pathname.split("/");
  return kind === "embed" || kind === "shorts" || kind === "live" ? id : undefined;
}

/** The Vimeo id in `url`: `vimeo.com/<id>` or `player.vimeo.com/video/<id>`. */
function vimeoId(url: URL): string | undefined {
  if (!VIMEO_HOSTS.has(url.hostname)) return undefined;
  const segments = url.pathname.split("/").filter(Boolean);
  if (url.hostname === "player.vimeo.com") {
    return segments[0] === "video" ? segments[1] : undefined;
  }
  return segments.length === 1 ? segments[0] : undefined;
}

/** The video `input` names, an `https` or `http` YouTube or Vimeo address, or `null`. */
export function videoFromUrl(input: string): VideoRef | null {
  let url: URL;
  try {
    url = new URL(input.trim());
  } catch {
    return null;
  }
  if (url.protocol !== "https:" && url.protocol !== "http:") return null;
  const youtube = youtubeId(url);
  if (youtube !== undefined && videoIdIsValid("youtube", youtube)) {
    return { provider: "youtube", video_id: youtube };
  }
  const vimeo = vimeoId(url);
  if (vimeo !== undefined && videoIdIsValid("vimeo", vimeo)) {
    return { provider: "vimeo", video_id: vimeo };
  }
  return null;
}
