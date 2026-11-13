// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Meta, StoryObj } from "@storybook/react-vite";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { Prose } from "./prose";

const meta = {
  title: "Foundations/Prose",
  component: Prose,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `Long-form content: lesson pages and descriptions, rendered from HTML the server sanitized. It sets the reading face (Source Serif 4 unless the organization chose sans), the reading size and line height, and a measure of about 66 characters, and styles the elements inside: headings, lists, links, quotes, code and tables. A wide table scrolls on its own.

**Usage.** \`<Prose>{content}</Prose>\`, where \`content\` is the lesson's elements.

**Do** keep one \`Prose\` per reading column, and use its headings for the content's own outline.

**Don't** put controls or app chrome inside it, or use mono for anything but code.`,
      },
    },
  },
  args: { children: null },
  argTypes: { children: { control: false } },
} satisfies Meta<typeof Prose>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Lesson: Story = {
  render: () => (
    <Prose>
      <h2>Taxi instructions</h2>
      <p>
        A taxi instruction gives the pilot a route to a runway or a parking position. It names the
        destination first, then the taxiways in order, and ends with any <strong>hold short</strong>{" "}
        instruction. Read the <a href="#phraseology">phraseology section</a> before the quiz.
      </p>
      <h3>What a complete instruction contains</h3>
      <ol>
        <li>The aircraft's call sign.</li>
        <li>The destination: a runway, a ramp or a gate.</li>
        <li>The route, taxiway by taxiway.</li>
        <li>Any hold short point, which the pilot must read back.</li>
      </ol>
      <blockquote>
        <p>A hold short instruction that is not read back has not been acknowledged.</p>
      </blockquote>
      <p>
        Write the route as letters and numbers, for example <code>A B3 C</code>, exactly as the
        airport diagram prints them.
      </p>
      <pre>
        <code>{"N123AB, runway 27, taxi via A, B3, hold short of runway 18."}</code>
      </pre>
      <hr />
      <p>When the pilot reads the instruction back, check each part against what you said.</p>
      <ul>
        <li>Call sign</li>
        <li>Runway</li>
        <li>Hold short point</li>
      </ul>
    </Prose>
  ),
};

/** A table wider than the measure scrolls on its own instead of widening the page. */
export const WideTable: Story = {
  render: () => (
    <Prose>
      <p>Each position, its frequency and what it controls:</p>
      <table>
        <thead>
          <tr>
            <th>Position</th>
            <th>Frequency</th>
            <th>Controls</th>
            <th>Hands off to</th>
            <th>Typical traffic</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td>Clearance delivery</td>
            <td>121.800</td>
            <td>IFR clearances before taxi</td>
            <td>Ground control</td>
            <td>Departures at the gate</td>
          </tr>
          <tr>
            <td>Ground control</td>
            <td>121.900</td>
            <td>Taxiways and ramps</td>
            <td>Local control</td>
            <td>Aircraft moving on the ground</td>
          </tr>
          <tr>
            <td>Local control</td>
            <td>118.300</td>
            <td>Runways and the airspace around them</td>
            <td>Departure control</td>
            <td>Takeoffs and landings</td>
          </tr>
        </tbody>
      </table>
    </Prose>
  ),
};

export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: () => (
    <ThemeMatrix>
      <Prose>
        <h3>Hold short</h3>
        <p>
          The pilot reads back every <strong>hold short</strong> instruction. See{" "}
          <a href="#readbacks">readbacks</a> and the route <code>A B3</code>.
        </p>
      </Prose>
    </ThemeMatrix>
  ),
};
