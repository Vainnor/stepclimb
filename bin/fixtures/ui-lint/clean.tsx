// SPDX-License-Identifier: AGPL-3.0-or-later
// A comment may name #fff, rgb(0 0 0) and text-blue-500 without failing.
const red = 1;
export const Row = () => (
  <div
    className="ms-2 pe-4 start-0 text-start rounded-s-md border-line bg-canvas text-fg-default"
    style={{ background: "var(--sc-bg-raised)" }}
    data-count={red}
  >
    <kbd>⌘K</kbd> Ctrl → next © 2026
  </div>
);
