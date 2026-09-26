import type { Context } from "hono";
import { setCookie } from "hono/cookie";
import * as jose from "jose";
import * as cookie from "cookie";
import { env } from "../lib/env";
import { getSessionCookieOptions } from "../lib/cookies";
import { Paths, Session } from "@contracts/constants";
import { Errors } from "@contracts/errors";
import { signSessionToken, verifySessionToken } from "./session";
import { users as kimiUsers } from "./platform";
import { findUserByUnionId, upsertUser } from "../queries/users";
import type { TokenResponse } from "./types";

// OAuth 一次性 nonce cookie(审计三轮 login-CSRF 防护)
const OAUTH_NONCE_COOKIE = "kimi_oauth_nonce";

async function exchangeAuthCode(
  code: string,
  redirectUri: string,
): Promise<TokenResponse> {
  const body = new URLSearchParams({
    grant_type: "authorization_code",
    code,
    client_id: env.appId,
    redirect_uri: redirectUri,
    client_secret: env.appSecret,
  });

  const resp = await fetch(`${env.kimiAuthUrl}/api/oauth/token`, {
    method: "POST",
    headers: { "Content-Type": "application/x-www-form-urlencoded" },
    body: body.toString(),
  });

  if (!resp.ok) {
    const text = await resp.text();
    throw new Error(`Token exchange failed (${resp.status}): ${text}`);
  }

  return resp.json() as Promise<TokenResponse>;
}

const jwks = jose.createRemoteJWKSet(
  new URL(`${env.kimiAuthUrl}/api/.well-known/jwks.json`),
);

async function verifyAccessToken(
  accessToken: string,
): Promise<{ userId: string; clientId: string }> {
  const { payload } = await jose.jwtVerify(accessToken, jwks);
  const userId = payload.user_id as string;
  const clientId = payload.client_id as string;
  if (!userId) {
    throw new Error("user_id missing from access token");
  }
  return { userId, clientId };
}

export async function authenticateRequest(headers: Headers) {
  const cookies = cookie.parse(headers.get("cookie") || "");
  const token = cookies[Session.cookieName];
  if (!token) {
    console.warn("[auth] No session cookie found in request.");
    throw Errors.forbidden("Invalid authentication token.");
  }
  const claim = await verifySessionToken(token);
  if (!claim) {
    throw Errors.forbidden("Invalid authentication token.");
  }
  const user = await findUserByUnionId(claim.unionId);
  if (!user) {
    throw Errors.forbidden("User not found. Please re-login.");
  }
  return user;
}

// 审计三轮中优: state 原本只是 btoa(redirectUri), 无会话绑定 → login-CSRF.
// 新格式 state = btoa(JSON({ru, n})): n 为 start 端点写入 HttpOnly cookie 的
// 一次性随机值, callback 校验 cookie.n === state.n. 旧格式(纯 b64)保持兼容
// 但无保护 — 集成方应改用 /api/oauth/start 发起.
export function createOAuthStartHandler() {
  return async (c: Context) => {
    const ru = c.req.query("redirect") || "/";
    if (ru.includes("://") || ru.startsWith("//")) {
      return c.json({ error: "redirect must be a relative path" }, 400);
    }
    const nonce = crypto.randomUUID() + crypto.randomUUID().replace(/-/g, "");
    const state = btoa(JSON.stringify({ ru, n: nonce }));
    const authorize = new URL(`${env.kimiAuthUrl}/api/oauth/authorize`);
    authorize.searchParams.set("response_type", "code");
    authorize.searchParams.set("client_id", env.appId);
    authorize.searchParams.set(
      "redirect_uri",
      new URL(Paths.oauthCallback, c.req.url).toString(),
    );
    authorize.searchParams.set("state", state);
    c.header(
      "set-cookie",
      cookie.serialize(OAUTH_NONCE_COOKIE, nonce, {
        httpOnly: true,
        sameSite: "lax",
        secure: getSessionCookieOptions(c.req.raw.headers).secure,
        path: "/",
        maxAge: 600,
      }),
    );
    return c.redirect(authorize.toString(), 302);
  };
}

export function createOAuthCallbackHandler() {
  return async (c: Context) => {
    const code = c.req.query("code");
    const state = c.req.query("state");
    const error = c.req.query("error");
    const errorDescription = c.req.query("error_description");

    if (error) {
      if (error === "access_denied") {
        return c.redirect("/", 302);
      }
      return c.json(
        { error, error_description: errorDescription },
        400,
      );
    }

    if (!code || !state) {
      return c.json({ error: "code and state are required" }, 400);
    }

    try {
      let redirectUri = state;
      // 新格式: 校验一次性 nonce(login-CSRF 防护)
      try {
        const parsed = JSON.parse(atob(state)) as { ru?: string; n?: string };
        if (parsed.ru && parsed.n) {
          const cookies = cookie.parse(c.req.raw.headers.get("cookie") || "");
          const expected = cookies[OAUTH_NONCE_COOKIE];
          if (!expected || expected !== parsed.n) {
            return c.json({ error: "state nonce mismatch" }, 400);
          }
          redirectUri = parsed.ru;
        }
        // 旧格式(纯 b64 redirectUri)保持兼容 — 无 CSRF 保护
      } catch {
        // 非 JSON → 旧格式
      }
      const tokenResp = await exchangeAuthCode(code, redirectUri);
      const { userId } = await verifyAccessToken(tokenResp.access_token);
      const userProfile = await kimiUsers.getProfile(tokenResp.access_token);
      if (!userProfile) {
        throw new Error("Failed to fetch user profile from Kimi Open");
      }

      await upsertUser({
        unionId: userId,
        name: userProfile.name,
        avatar: userProfile.avatar_url,
        lastSignInAt: new Date(),
      });

      const token = await signSessionToken({
        unionId: userId,
        clientId: env.appId,
      });

      const cookieOpts = getSessionCookieOptions(c.req.raw.headers);
      setCookie(c, Session.cookieName, token, {
        ...cookieOpts,
        maxAge: Session.maxAgeMs / 1000,
      });

      return c.redirect("/", 302);
    } catch (error) {
      console.error("[OAuth] Callback failed", error);
      return c.json({ error: "OAuth callback failed" }, 500);
    }
  };
}

export { exchangeAuthCode, verifyAccessToken };
