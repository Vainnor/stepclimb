// SPDX-License-Identifier: AGPL-3.0-or-later
import "@fontsource/atkinson-hyperlegible-next/400.css";
import "@fontsource/atkinson-hyperlegible-next/500.css";
import "@fontsource/atkinson-hyperlegible-next/700.css";
import "./styles.css";
import { StringsProvider } from "@stepclimb/ui";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { AppRoot, createApp } from "./app";
import { catalogs } from "./messages";

const root = document.getElementById("root");
if (!root) throw new Error("index.html has no #root element");

const app = createApp();

createRoot(root).render(
  <StrictMode>
    <StringsProvider catalogs={catalogs} isDevelopment={import.meta.env.DEV}>
      <AppRoot app={app} />
    </StringsProvider>
  </StrictMode>,
);
