// The web client: one page, its screens chosen by the router (`router.tsx`). No StrictMode: it
// runs effects twice in development, and the player's effect starts a game session.
import "./styles/global.css";
import { RouterProvider } from "@tanstack/react-router";
import { createRoot } from "react-dom/client";
import { router } from "./router";

createRoot(document.getElementById("root")!).render(<RouterProvider router={router} />);
