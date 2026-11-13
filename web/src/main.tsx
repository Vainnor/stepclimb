// SPDX-License-Identifier: AGPL-3.0-or-later
import "@fontsource/atkinson-hyperlegible-next/400.css";
import "@fontsource/atkinson-hyperlegible-next/500.css";
import "@fontsource/atkinson-hyperlegible-next/700.css";
import "./styles.css";
import { StringsProvider } from "@stepclimb/ui";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { RouterProvider } from "@tanstack/react-router";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { catalogs } from "./messages";
import { createAppRouter } from "./router";

const root = document.getElementById("root");
if (!root) throw new Error("index.html has no #root element");

const queryClient = new QueryClient();
const router = createAppRouter();

createRoot(root).render(
  <StrictMode>
    <StringsProvider catalogs={catalogs} isDevelopment={import.meta.env.DEV}>
      <QueryClientProvider client={queryClient}>
        <RouterProvider router={router} />
      </QueryClientProvider>
    </StringsProvider>
  </StrictMode>,
);
