// SPDX-License-Identifier: AGPL-3.0-or-later
// How a token's id becomes its CSS variable and its Tailwind theme key. The CSS build, the
// Tailwind theme and the docs tables all name tokens through these.

/** `color.bg-canvas` -> `--sc-bg-canvas`; `radius.sm` -> `--sc-radius-sm`. */
export function variableName(id: string): string {
  const [group, ...rest] = id.split(".");
  return `--sc-${(group === "color" ? rest : [group, ...rest]).join("-")}`;
}

/** The Tailwind 4 theme namespace of each token group; groups not listed have no theme key. */
const namespaces: Record<string, string> = {
  color: "color",
  radius: "radius",
  text: "text",
  leading: "leading",
  weight: "font-weight",
  font: "font",
  ease: "ease",
  space: "spacing",
  shadow: "shadow",
};

/**
 * A token's key in Tailwind's theme: `color.bg-canvas` -> `color`, `canvas` (`--color-canvas`).
 * Surfaces drop their `bg-` so the utilities read `bg-canvas`, `bg-overlay`, `ring-offset-canvas`.
 * Undefined for groups Tailwind's theme has no namespace for (motion, control heights).
 */
export function tailwindKey(id: string): { namespace: string; name: string } | undefined {
  const [group, ...rest] = id.split(".");
  const namespace = group ? namespaces[group] : undefined;
  if (!namespace) return undefined;
  const name = group === "color" ? rest.join("-").replace(/^bg-/, "") : rest.join("-");
  return { namespace, name };
}
