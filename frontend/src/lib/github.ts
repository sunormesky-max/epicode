// ── GitHub repository data for the community page ──
//
// Fetches live stats (stars, forks, issues, contributors) directly from the
// public GitHub REST API. Unauthenticated requests are rate-limited to 60/hour
// per IP, so results are cached both in-memory and in sessionStorage (TTL 10m)
// to avoid hammering the API on every page visit.
//
// All calls are best-effort: any failure resolves to `null` so the UI can
// degrade gracefully instead of showing an error state.

export const GITHUB_OWNER = 'sunormesky-max';
export const GITHUB_REPO = 'epicode';
export const REPO_SLUG = `${GITHUB_OWNER}/${GITHUB_REPO}`;
const API_BASE = `https://api.github.com/repos/${REPO_SLUG}`;
const REPO_URL = `https://github.com/${REPO_SLUG}`;

// Number of contributors to fetch for the avatar grid.
const MAX_CONTRIBUTORS = 12;
// Abort any request that takes longer than this.
const REQUEST_TIMEOUT_MS = 8000;
// Cache TTL shared by memory and sessionStorage layers.
const CACHE_TTL_MS = 10 * 60 * 1000;
const STORAGE_KEY = 'epicode_github_cache_v1';

export interface GitHubRepoStats {
  stars: number;
  forks: number;
  openIssues: number;
  watchers: number;
}

export interface GitHubContributor {
  login: string;
  avatarUrl: string;
  htmlUrl: string;
  contributions: number;
}

export interface GitHubData {
  stats: GitHubRepoStats;
  contributors: GitHubContributor[];
  repoUrl: string;
}

// ── Cache ──

interface CacheEntry {
  data: GitHubData;
  ts: number;
}

let memoryCache: CacheEntry | null = null;

function readStorageCache(): CacheEntry | null {
  try {
    const raw = sessionStorage.getItem(STORAGE_KEY);
    if (!raw) return null;
    const entry = JSON.parse(raw) as CacheEntry;
    if (Date.now() - entry.ts > CACHE_TTL_MS) return null;
    return entry;
  } catch {
    return null;
  }
}

function writeStorageCache(entry: CacheEntry): void {
  try {
    sessionStorage.setItem(STORAGE_KEY, JSON.stringify(entry));
  } catch {
    // sessionStorage may be unavailable (private mode / quota); ignore.
  }
}

function isCacheFresh(entry: CacheEntry | null): entry is CacheEntry {
  return !!entry && Date.now() - entry.ts < CACHE_TTL_MS;
}

// ── Fetch helper with timeout ──

async function fetchJSON<T>(url: string): Promise<T> {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), REQUEST_TIMEOUT_MS);
  try {
    const res = await fetch(url, {
      signal: controller.signal,
      headers: { Accept: 'application/vnd.github+json' },
    });
    if (!res.ok) {
      throw new Error(`GitHub API ${res.status} for ${url}`);
    }
    return (await res.json()) as T;
  } finally {
    clearTimeout(timer);
  }
}

// ── Public API ──

/**
 * Fetch live GitHub stats + contributors for the Epicode repo.
 * Returns cached data when fresh, and always resolves (null on failure).
 */
export async function getGitHubData(): Promise<GitHubData | null> {
  // In-memory cache first (instant for repeat visits in the same session).
  if (isCacheFresh(memoryCache)) return memoryCache.data;

  // Then sessionStorage (survives a page reload within the TTL).
  const stored = readStorageCache();
  if (isCacheFresh(stored)) {
    memoryCache = stored;
    return stored.data;
  }

  try {
    interface RepoApiResponse {
      stargazers_count: number;
      forks_count: number;
      open_issues_count: number;
      subscribers_count: number;
    }
    interface ContributorApiResponse {
      login: string;
      avatar_url: string;
      html_url: string;
      contributions: number;
    }

    const [repo, contributors] = await Promise.all([
      fetchJSON<RepoApiResponse>(API_BASE),
      fetchJSON<ContributorApiResponse[]>(
        `${API_BASE}/contributors?per_page=${MAX_CONTRIBUTORS}&anon=false`,
      ),
    ]);

    const data: GitHubData = {
      stats: {
        stars: repo.stargazers_count ?? 0,
        forks: repo.forks_count ?? 0,
        openIssues: repo.open_issues_count ?? 0,
        watchers: repo.subscribers_count ?? 0,
      },
      contributors: (contributors ?? []).map((c) => ({
        login: c.login,
        avatarUrl: c.avatar_url,
        htmlUrl: c.html_url,
        contributions: c.contributions ?? 0,
      })),
      repoUrl: REPO_URL,
    };

    const entry: CacheEntry = { data, ts: Date.now() };
    memoryCache = entry;
    writeStorageCache(entry);
    return data;
  } catch (err) {
    // Rate limit, network error, or timeout — surface once, then degrade.
    console.warn('[epicode] GitHub data unavailable:', err);
    return null;
  }
}

/** Convenience accessor for the raw repository URL (used in CTAs/links). */
export function getRepoUrl(): string {
  return REPO_URL;
}

/** Well-known community links derived from the repo slug. */
export const GitHubLinks = {
  repo: REPO_URL,
  issues: `${REPO_URL}/issues`,
  pulls: `${REPO_URL}/pulls`,
  discussions: `${REPO_URL}/discussions`,
  sponsors: `https://github.com/sponsors/${GITHUB_OWNER}`,
  contributors: `${REPO_URL}/graphs/contributors`,
  contributing: `${REPO_URL}/blob/main/CONTRIBUTING.md`,
  coc: `${REPO_URL}/blob/main/CODE_OF_CONDUCT.md`,
  security: `${REPO_URL}/security/advisories/new`,
  governance: `${REPO_URL}/blob/main/.github/GOVERNANCE.md`,
} as const;
