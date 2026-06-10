import { authRouter } from "./auth-router";
import { notesRouter } from "./notes-router";
import { createRouter, publicQuery } from "./middleware";

export const appRouter = createRouter({
  ping: publicQuery.query(() => ({ ok: true, ts: Date.now() })),
  auth: authRouter,
  notes: notesRouter,
});

export type AppRouter = typeof appRouter;
