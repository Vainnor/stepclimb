// SPDX-License-Identifier: AGPL-3.0-or-later
// The Tokens page's tables, read from the docs tables packages/tokens builds
// (`@stepclimb/tokens/docs/tokens.json`, spec 12), so the page cannot drift from the CSS. Each color
// value has a swatch drawn by its CSS variable under that theme's `data-sc-theme`.
import tables from "@stepclimb/tokens/docs/tokens.json";

const appearances = ["light", "dark", "high-contrast"] as const;
type Appearance = (typeof appearances)[number];
const appearanceTitles: Record<Appearance, string> = {
  light: "Light",
  dark: "Dark",
  "high-contrast": "High contrast",
};

type Token = (typeof tables.groups)[number]["tokens"][number];

function Value({ token, appearance }: { token: Token; appearance: Appearance }) {
  const value = token.values[appearance];
  if (token.type !== "color") return <code>{value}</code>;
  return (
    <span className="inline-flex items-center gap-2">
      <span data-sc-theme={appearance} className="inline-flex rounded-xs bg-canvas p-1">
        <span
          aria-hidden="true"
          className="inline-block size-6 rounded-xs border border-line-strong"
          style={{ background: `var(${token.variable})` }}
        />
      </span>
      <code>{value}</code>
    </span>
  );
}

function GroupTable({ group }: { group: (typeof tables.groups)[number] }) {
  const compact = group.tokens.some((t) => t.compact !== null);
  return (
    <section>
      <h2>{group.title}</h2>
      <table>
        <thead>
          <tr>
            <th scope="col">Token</th>
            <th scope="col">Tailwind</th>
            <th scope="col">Description</th>
            {appearances.map((a) => (
              <th scope="col" key={a}>
                {appearanceTitles[a]}
              </th>
            ))}
            {compact ? <th scope="col">Compact</th> : null}
          </tr>
        </thead>
        <tbody>
          {group.tokens.map((t) => (
            <tr key={t.id}>
              <th scope="row">
                <code>{t.variable}</code>
              </th>
              <td>
                <code>{t.utility}</code>
              </td>
              <td>{t.alias ? `${t.description} (alias of ${t.alias})` : t.description}</td>
              {appearances.map((a) => (
                <td key={a}>
                  <Value token={t} appearance={a} />
                </td>
              ))}
              {compact ? <td>{t.compact === null ? null : <code>{t.compact}</code>}</td> : null}
            </tr>
          ))}
        </tbody>
      </table>
    </section>
  );
}

/** Every group of the plan's Tokens tables, with the default brand's values. */
export function TokensTables() {
  return (
    <>
      {tables.groups.map((g) => (
        <GroupTable key={g.title} group={g} />
      ))}
    </>
  );
}
