import { z } from "zod";

const timestamp = z.iso.datetime();
const sessionID = z
  .string()
  .regex(/^[a-f0-9]{32}$/)
  .brand<"SessionID">();
const sessionSchema = z.object({
  id: sessionID,
  createdAt: timestamp,
  expiresAt: timestamp,
});
const csrfToken = z
  .string()
  .regex(/^[A-Za-z0-9_-]{43}$/)
  .brand<"CSRFToken">();
const sessionViewSchema = z.object({
  owner: z.object({ username: z.string() }),
  session: sessionSchema,
  csrfToken,
});
const botSchema = z.object({
  id: z
    .string()
    .regex(/^[a-f0-9]{32}$/)
    .brand<"BotID">(),
  name: z.string(),
  createdAt: timestamp,
});
const sessionItemSchema = sessionSchema.extend({ current: z.boolean() });
const errorSchema = z.object({
  error: z.object({ code: z.string(), message: z.string() }),
});

export type SessionView = z.infer<typeof sessionViewSchema>;
export type Bot = z.infer<typeof botSchema>;
export type SessionItem = z.infer<typeof sessionItemSchema>;
export type CSRFToken = SessionView["csrfToken"];
export type Load<T> =
  | { kind: "loading" }
  | { kind: "ready"; value: T }
  | { kind: "failed"; message: string };
export type Connection =
  { kind: "connecting" } | { kind: "connected" } | { kind: "disconnected" };
export type Auth =
  | { kind: "checking" }
  | { kind: "signedOut"; reason: "initial" | "ended" | "signedOut" | "revoked" }
  | { kind: "signedIn"; session: SessionView };

export class ApiError extends Error {
  constructor(
    public readonly code: string,
    message: string,
    public readonly status: number,
  ) {
    super(message);
  }
}

async function request<Schema extends z.ZodType>({
  path,
  schema,
  signal,
  method = "GET",
  body,
  csrf,
}: {
  path: string;
  schema: Schema;
  signal?: AbortSignal;
  method?: "GET" | "POST" | "DELETE";
  body?: object;
  csrf?: CSRFToken;
}): Promise<z.output<Schema>> {
  const headers = new Headers();
  if (method !== "GET") {
    headers.set("Content-Type", "application/json");
    headers.set("X-Botstral-Request", "1");
    if (csrf) headers.set("X-CSRF-Token", csrf);
  }
  const response = await fetch(`/api/v1${path}`, {
    method,
    credentials: "same-origin",
    cache: "no-store",
    headers,
    body: body ? JSON.stringify(body) : undefined,
    signal: signal
      ? AbortSignal.any([signal, AbortSignal.timeout(8000)])
      : AbortSignal.timeout(8000),
  });
  let payload: unknown = null;
  if (response.status !== 204) {
    try {
      payload = await response.json();
    } catch {
      throw new ApiError(
        "invalid_response",
        "The manager returned an unreadable response.",
        response.status,
      );
    }
  }
  if (!response.ok) {
    const decoded = errorSchema.safeParse(payload);
    if (decoded.success)
      throw new ApiError(
        decoded.data.error.code,
        decoded.data.error.message,
        response.status,
      );
    throw new ApiError(
      "invalid_response",
      "The manager returned an unreadable response.",
      response.status,
    );
  }
  const decoded = schema.safeParse(payload);
  if (!decoded.success)
    throw new ApiError(
      "invalid_response",
      "The manager returned an unreadable response.",
      response.status,
    );
  return decoded.data;
}

export const api = {
  health: (signal?: AbortSignal) =>
    request({
      path: "/health",
      schema: z.object({ status: z.literal("ok") }),
      signal,
    }),
  session: (signal?: AbortSignal) =>
    request({ path: "/session", schema: sessionViewSchema, signal }),
  signIn: (body: { username: string; password: string }, signal: AbortSignal) =>
    request({
      path: "/sign-in",
      schema: sessionViewSchema,
      method: "POST",
      body,
      signal,
    }),
  signOut: (csrf: CSRFToken, signal: AbortSignal) =>
    request({
      path: "/sign-out",
      schema: z.null(),
      method: "POST",
      body: {},
      csrf,
      signal,
    }),
  listBots: (signal: AbortSignal) =>
    request({
      path: "/bots",
      schema: z.object({ bots: z.array(botSchema) }),
      signal,
    }).then((value) => value.bots),
  createBot: (body: { name: string }, csrf: CSRFToken, signal: AbortSignal) =>
    request({
      path: "/bots",
      schema: botSchema,
      method: "POST",
      body,
      csrf,
      signal,
    }),
  listSessions: (signal: AbortSignal) =>
    request({
      path: "/sessions",
      schema: z.object({ sessions: z.array(sessionItemSchema) }),
      signal,
    }).then((value) => value.sessions),
  revokeSession: (
    id: SessionItem["id"],
    csrf: CSRFToken,
    signal: AbortSignal,
  ) =>
    request({
      path: `/sessions/${id}`,
      schema: z.null(),
      method: "DELETE",
      body: {},
      csrf,
      signal,
    }),
};

export function errorMessage(error: unknown): string {
  return error instanceof ApiError
    ? error.message
    : "Could not reach the manager. Check the connection and try again.";
}

export function formatDate(value: string): string {
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(new Date(value));
}
