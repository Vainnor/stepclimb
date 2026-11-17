// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Meta, StoryObj } from "@storybook/react-vite";
import { serverHtml } from "../lib/server-html";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { Announcement } from "./announcement";

const body = [
  "<p>The <strong>ground control</strong> module opens on Monday. Finish clearance delivery first;",
  ' the <a href="#readbacks">readback drill</a> is a good warm-up.</p>',
  "<ul><li>Bring a headset to the session.</li><li>Read the taxi chart beforehand.</li></ul>",
].join("");
const html = serverHtml(body);

const meta = {
  title: "Learning/Announcement",
  component: Announcement,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `A message staff posted to a course or an account: its title, who posted it and when, and its body. The body is the HTML the server rendered and sanitized, drawn in the reading face; the date reads in the user's time zone.

**Usage.** \`<Announcement title={a.title} author={a.author.display_name} publishedAt={new Date(a.published_at)} timeZone={me.tz} html={serverHtml(a.html)} />\`, where \`serverHtml()\` marks the API's server-rendered field. Set \`headingLevel\` to the title's place in the page.

**Do** pass the server's \`html\` as it came.

**Don't** call \`serverHtml()\` on HTML built or edited in the browser: it is inserted as given.`,
      },
    },
  },
  args: {
    title: "Ground control opens on Monday",
    author: "Pat Lee",
    publishedAt: new Date("2026-03-04T03:00:00Z"),
    timeZone: "America/New_York",
    html,
  },
} satisfies Meta<typeof Announcement>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};

export const LongContent: Story = {
  args: {
    className: "max-w-xs",
    title: "Changes to the approach control course after the airspace redesign and new procedures",
    author: "Alexandria Montgomery-Vasquez",
    html: serverHtml(
      `${body}<p>Coordination procedures with adjacent facilities changed on 1 March; the updated letters are in the course files.</p>`,
    ),
  },
};

export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: (args) => (
    <ThemeMatrix>
      <Announcement {...args} />
    </ThemeMatrix>
  ),
};
