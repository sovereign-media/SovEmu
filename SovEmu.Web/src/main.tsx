// The web client: one page, its screens chosen by the router (`router.tsx`). No StrictMode: it
// runs effects twice in development, and the player's effect starts a game session.
import "./styles/global.css";
import { QueryClientProvider } from "@tanstack/react-query";
import { RouterProvider } from "@tanstack/react-router";
import { createRoot } from "react-dom/client";
import { queryClient } from "./lib/data";
import { router } from "./router";

createRoot(document.getElementById("root")!).render(
  <QueryClientProvider client={queryClient}>
    <RouterProvider router={router} />
  </QueryClientProvider>,
);
