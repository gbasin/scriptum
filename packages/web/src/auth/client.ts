// HTTP client for relay auth endpoints.

export interface OAuthStartResult {
  flow_id: string;
  authorization_url: string;
  expires_at: string;
}

export interface OAuthCallbackResult {
  access_token: string;
  access_expires_at: string;
  refresh_token: string;
  refresh_expires_at: string;
  user: AuthUser;
}

export interface AuthUser {
  id: string;
  email: string;
  display_name: string;
}

export interface RefreshResult {
  access_token: string;
  access_expires_at: string;
  refresh_token: string;
  refresh_expires_at: string;
}

export interface AuthClientOptions {
  baseUrl: string;
}

export class AuthClient {
  private readonly baseUrl: string;

  constructor(options: AuthClientOptions) {
    this.baseUrl = options.baseUrl.replace(/\/+$/, "");
  }

  /** POST /v1/auth/oauth/github/start — initiate OAuth flow. */
  async startOAuth(params: {
    redirect_uri: string;
    state: string;
    code_challenge: string;
    code_challenge_method: string;
  }): Promise<OAuthStartResult> {
    const res = await fetch(`${this.baseUrl}/v1/auth/oauth/github/start`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(params),
    });
    if (!res.ok) {
      throw new AuthClientError(res.status, await safeText(res));
    }
    return res.json() as Promise<OAuthStartResult>;
  }

  /** POST /v1/auth/oauth/github/callback — exchange code for tokens. */
  async exchangeCode(params: {
    flow_id: string;
    code: string;
    state: string;
    code_verifier: string;
  }): Promise<OAuthCallbackResult> {
    const res = await fetch(`${this.baseUrl}/v1/auth/oauth/github/callback`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(params),
    });
    if (!res.ok) {
      throw new AuthClientError(res.status, await safeText(res));
    }
    return res.json() as Promise<OAuthCallbackResult>;
  }

  /** POST /v1/auth/token/refresh — rotate refresh token. */
  async refreshToken(refreshToken: string): Promise<RefreshResult> {
    const res = await fetch(`${this.baseUrl}/v1/auth/token/refresh`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ refresh_token: refreshToken }),
    });
    if (!res.ok) {
      throw new AuthClientError(res.status, await safeText(res));
    }
    return res.json() as Promise<RefreshResult>;
  }

  /** POST /v1/auth/logout — revoke session. */
  async logout(refreshToken: string): Promise<void> {
    const res = await fetch(`${this.baseUrl}/v1/auth/logout`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ refresh_token: refreshToken }),
    });
    if (!res.ok) {
      throw new AuthClientError(res.status, await safeText(res));
    }
  }
}

export class AuthClientError extends Error {
  constructor(
    public readonly status: number,
    public readonly body: string,
  ) {
    super(`Auth request failed (${status}): ${body}`);
    this.name = "AuthClientError";
  }
}

async function safeText(res: Response): Promise<string> {
  try {
    return await res.text();
  } catch {
    return "";
  }
}
