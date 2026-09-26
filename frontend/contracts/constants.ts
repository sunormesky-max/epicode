export const Session = {
  cookieName: "kimi_sid",
  maxAgeMs: 7 * 24 * 60 * 60 * 1000, // 审计三轮: 会话7天(原一年)
} as const;

export const ErrorMessages = {
  unauthenticated: "Authentication required",
  insufficientRole: "Insufficient permissions",
} as const;

export const Paths = {
  login: "/login",
  oauthCallback: "/api/oauth/callback",
  oauthStart: "/api/oauth/start",
} as const;
