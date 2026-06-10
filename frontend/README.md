# Personal Notes Fullstack Template

A fullstack personal notes / knowledge-base app with wiki-style `[[links]]`, Markdown rendering, a D3-powered knowledge graph view, a real-time moon-phase widget, and four cinematic backgrounds (moonlit ripple, silk flow field, rain on glass, solid color). Per-user note persistence with automatic starter-notes seeding on first sign-in.

## Features

- Markdown editor with live preview (GFM: tables, code blocks, task lists)
- Wiki-style `[[links]]` between notes — clicking navigates to or auto-creates the target note
- D3 force-directed knowledge graph view over all notes + links
- Four procedural backgrounds: moonlit ripple, silk flow field, rain on glass, and a solid-color picker
- Real-time moon-phase widget (canvas-rendered)
- Liquid-glass morphism UI (frosted panels)
- Auth-gated per-user persistence via tRPC + MySQL; localStorage fallback when unauthenticated
- Starter notes auto-seeded into MySQL on first sign-in

## Tech Stack

- React 19 + TypeScript + Vite
- Tailwind CSS v3 + shadcn/ui
- tRPC 11 + Hono + Drizzle ORM + MySQL
- Kimi OAuth 2.0 authentication
- D3.js (knowledge graph)
- `react-markdown` + `remark-gfm`
- React Router v7

## Quick Start

1. Clone / extract this template
2. Install dependencies: `npm install`
3. Copy `.env.example` to `.env` and fill in `DATABASE_URL` and Kimi OAuth credentials
4. Run database migrations: `npx drizzle-kit push`
5. (Optional) Seed starter notes for a specific user: `npx tsx db/seed.ts <userId>`
6. Run the dev server: `npm run dev`
7. Build for production: `npm run build`

## Configuration

Content is edited in two places — do not modify component files:

- **`src/config.ts`** — site title, header labels, background options, sidebar / editor / graph UI strings, moon phase labels, and `starterNotes` (shown to unauthenticated users and as a local fallback)
- **`api/notes-router.ts`** — the server-side `STARTER_NOTES` array, auto-seeded into MySQL for each new user on first login

See `info.md` (outer folder) for every config field with constraints.

## Database Schema

Two tables, defined in `db/schema.ts`:

- **`users`** — Kimi OAuth-managed (id, unionId, name, email, avatar, role)
- **`notes`** — user-owned notes (id, userId, title, content, tags, source, createdAt, updatedAt)

Seed script: `npx tsx db/seed.ts <userId>`

## Required Assets

No images or videos required — every background is procedural (WebGL / Canvas). Content is plain Markdown.

## Project Structure

```
.
├── api/                # tRPC routers, Hono server, Kimi OAuth, notes router with STARTER_NOTES
├── contracts/          # Shared tRPC types between server and client
├── db/                 # Drizzle schema, migrations, seed
├── public/             # Static assets
├── src/
│   ├── components/     # UI components (editor, sidebar, graph, backgrounds, moon widget)
│   ├── hooks/          # Custom hooks (notes, auth)
│   ├── config.ts       # All editable UI strings and starter notes (client)
│   ├── store.ts        # localStorage fallback store
│   └── App.tsx         # Root component
├── Dockerfile
├── drizzle.config.ts
├── .backend-features.json  # Declares ["auth", "db"]
└── .env.example
```

## Design

- Background: `#000000` or one of four cinematic WebGL / Canvas modes
- Foreground text: `#e0e0e0`
- Accent: `#c8956c` (warm amber) · Link color: `#d4a574`
- UI: liquid-glass frosted panels

## Notes

- Don't modify component logic unless fixing a real bug
- All UI copy lives in `src/config.ts`; server-side starter content lives in `api/notes-router.ts`
- Wiki-link target titles must match a note title (case-insensitive) for resolution to work
- The `src/store.ts` localStorage fallback is intentional — it lets the app work for unauthenticated visitors and gracefully hands off to the server store on login
