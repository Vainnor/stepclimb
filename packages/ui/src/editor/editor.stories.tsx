// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, within } from "storybook/test";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { Editor } from "./editor";
import type { PageDoc } from "./page-doc";

const text = (value: string, marks?: { type: string; attrs?: Record<string, unknown> }[]) =>
  marks ? { type: "text", text: value, marks } : { type: "text", text: value };
const paragraph = (...content: ReturnType<typeof text>[]) => ({ type: "paragraph", content });

/** A page with one of every block the model allows. */
const page: PageDoc = {
  type: "doc",
  content: [
    { type: "heading", attrs: { level: 2 }, content: [text("Before you taxi")] },
    paragraph(
      text("Read back every "),
      text("hold short", [{ type: "bold" }]),
      text(" instruction in full. The "),
      text("phraseology guide", [{ type: "link", attrs: { href: "/c/x" } }]),
      text(" lists the exact words."),
    ),
    {
      type: "callout",
      attrs: { kind: "warning" },
      content: [paragraph(text("Two runways in this module cross each other."))],
    },
    {
      type: "bulletList",
      content: [
        { type: "listItem", content: [paragraph(text("Request taxi with the ATIS code"))] },
        { type: "listItem", content: [paragraph(text("Read back the route and hold points"))] },
      ],
    },
    {
      type: "transcript",
      content: [
        {
          type: "transcriptLine",
          attrs: { speaker: "Ground" },
          content: [text("Taxi to runway 1 via Alpha, hold short of runway 6.")],
        },
        {
          type: "transcriptLine",
          attrs: { speaker: "Aircraft" },
          content: [text("Runway 1 via Alpha, hold short of runway 6.")],
        },
      ],
    },
    {
      type: "table",
      content: [
        {
          type: "tableRow",
          content: ["Point", "Frequency"].map((head) => ({
            type: "tableHeader",
            attrs: { colspan: 1, rowspan: 1, colwidth: null },
            content: [paragraph(text(head))],
          })),
        },
        {
          type: "tableRow",
          content: ["Ground", "121.9"].map((cell) => ({
            type: "tableCell",
            attrs: { colspan: 1, rowspan: 1, colwidth: null },
            content: [paragraph(text(cell))],
          })),
        },
      ],
    },
    {
      type: "codeBlock",
      attrs: { language: "json" },
      content: [text('{ "runway": "1", "hold_short": "6" }')],
    },
    {
      type: "image",
      attrs: {
        course_file_id: "01900000-0000-7000-8000-0000000000f1",
        alt: "The taxiway diagram",
        decorative: false,
      },
    },
    {
      type: "videoEmbed",
      attrs: { provider: "youtube", video_id: "dQw4w9WgXcQ", title: "A taxi briefing" },
    },
    { type: "horizontalRule" },
    paragraph(text("That is all for this page.")),
  ],
};

const meta = {
  title: "Authoring/Editor",
  component: Editor,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["States", "Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `The page editor: a Tiptap editor that holds only the blocks a page may hold (headings 2 to 4, lists, quotes, code, tables, callouts, transcripts, images, video embeds, dividers) and gives the body in the form the server stores. The toolbar sets the text style, marks, links and lists and opens "Insert"; typing \`/\` at the start of an empty paragraph opens the same list. The bar under it holds the settings of the block the caret is in: a callout's kind, an image's text alternative, a transcript line's speaker, a table's rows and columns, and moving or deleting the block.

**Usage.** \`<Editor value={body} onChange={setBody} onUploadImage={upload} problem={refusal} />\`. It saves nothing: the screen around it does, and passes the server's refusal back as \`problem\`, which the editor shows above the page and at its block.

**Do** pass \`onUploadImage\` where the page's course takes uploads; without it, "Image" is not offered.

**Don't** put the editor in a modal: an author works in it for minutes at a time.`,
      },
    },
  },
  args: { defaultValue: page, className: "max-w-4xl" },
} satisfies Meta<typeof Editor>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};

export const Empty: Story = {
  args: { defaultValue: { type: "doc", content: [{ type: "paragraph" }] } },
};

/** A refusal from the server, shown above the page and outlined at the image it names. */
export const WithProblem: Story = {
  args: {
    problem: {
      message: "This file is not a PNG, JPEG, GIF or WebP image. Replace it, then save again.",
      code: "page.image_type",
      block: 7,
    },
  },
};

/** Read only, as while a save is in flight. */
export const Disabled: Story = { args: { isDisabled: true } };

/** The block list, opened from "Insert". */
export const InsertMenu: Story = {
  args: { defaultValue: { type: "doc", content: [{ type: "paragraph" }] } },
  play: async () => {
    const body = within(document.body);
    await userEvent.click(await body.findByRole("button", { name: "Insert" }));
    await expect(await body.findByRole("menuitem", { name: "Callout" })).toBeVisible();
  },
};

/** The link popover from the toolbar, refusing an address the server would refuse. */
export const LinkPopover: Story = {
  play: async () => {
    const body = within(document.body);
    await userEvent.click(await body.findByRole("button", { name: "Link" }));
    const dialog = await body.findByRole("dialog", { name: "Link" });
    await userEvent.type(within(dialog).getByRole("textbox", { name: /Address/ }), "javascript:x");
    await userEvent.click(within(dialog).getByRole("button", { name: "Apply" }));
    await expect(await within(dialog).findByText(/Enter an address/)).toBeVisible();
  },
};

/** The block bar for an image without a text alternative: the field asks for one. */
export const ImageBlock: Story = {
  args: {
    defaultValue: {
      type: "doc",
      content: [
        {
          type: "image",
          attrs: {
            course_file_id: "01900000-0000-7000-8000-0000000000f1",
            alt: "",
            decorative: false,
          },
        },
        { type: "paragraph" },
      ],
    },
  },
};

/** The block bar for a table: rows, columns and the header row. */
export const TableBlock: Story = {
  args: {
    defaultValue: {
      type: "doc",
      content: [page.content[5]].filter(
        (block): block is NonNullable<typeof block> => block !== undefined,
      ),
    },
  },
};

export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: () => (
    <ThemeMatrix>
      <Editor
        defaultValue={{
          type: "doc",
          content: [page.content[0], page.content[1], page.content[2]].filter(
            (block): block is NonNullable<typeof block> => block !== undefined,
          ),
        }}
      />
    </ThemeMatrix>
  ),
};
