// SPDX-License-Identifier: AGPL-3.0-or-later
import { videoFromUrl, videoIdIsValid } from "./video-url";

describe("videoFromUrl", () => {
  it.each([
    ["https://www.youtube.com/watch?v=dQw4w9WgXcQ", "youtube", "dQw4w9WgXcQ"],
    ["https://youtube.com/watch?v=dQw4w9WgXcQ&t=42", "youtube", "dQw4w9WgXcQ"],
    ["https://m.youtube.com/watch?v=dQw4w9WgXcQ", "youtube", "dQw4w9WgXcQ"],
    ["https://youtu.be/dQw4w9WgXcQ?si=x", "youtube", "dQw4w9WgXcQ"],
    ["https://www.youtube.com/embed/dQw4w9WgXcQ", "youtube", "dQw4w9WgXcQ"],
    ["https://www.youtube-nocookie.com/embed/dQw4w9WgXcQ", "youtube", "dQw4w9WgXcQ"],
    ["https://www.youtube.com/shorts/dQw4w9WgXcQ", "youtube", "dQw4w9WgXcQ"],
    ["  https://vimeo.com/76979871  ", "vimeo", "76979871"],
    ["https://player.vimeo.com/video/76979871?h=abc", "vimeo", "76979871"],
  ])("reads %s", (url, provider, video_id) => {
    expect(videoFromUrl(url)).toEqual({ provider, video_id });
  });

  it.each([
    "dQw4w9WgXcQ",
    "https://www.youtube.com/watch?v=dQw4w9WgXcQx",
    "https://www.youtube.com/watch?v=dQw4w9WgXc",
    "https://evil.example/watch?v=dQw4w9WgXcQ",
    "https://youtube.com.evil.example/watch?v=dQw4w9WgXcQ",
    "javascript:alert(1)//youtu.be/dQw4w9WgXcQ",
    "https://vimeo.com/channels/staffpicks/76979871",
    "https://vimeo.com/abc",
    "https://player.vimeo.com/76979871",
    "ftp://youtu.be/dQw4w9WgXcQ",
  ])("refuses %s", (url) => {
    expect(videoFromUrl(url)).toBeNull();
  });

  it("checks each provider's id format", () => {
    expect(videoIdIsValid("youtube", "dQw4w9WgXcQ")).toBe(true);
    expect(videoIdIsValid("youtube", "dQw4w9WgXcQ1")).toBe(false);
    expect(videoIdIsValid("vimeo", "1".repeat(20))).toBe(true);
    expect(videoIdIsValid("vimeo", "1".repeat(21))).toBe(false);
  });
});
