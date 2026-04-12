# Cloud Upload Service - Architecture Design

**Status:** Planned
**Created:** 2026-04-12
**Priority:** Future Feature

## Overview

ApexShot will offer a hybrid cloud upload system:
1. **ApexShot Cloud** — Hosted service with freemium subscriptions (revenue stream)
2. **Self-hosted** — Users configure their own backend (zero liability option)

This document covers the full architecture for both paths.

---

## Security Model

### Threat Landscape

| Threat | Mitigation |
|--------|------------|
| Hardcoded credentials extracted from malicious forks | OAuth 2.0 with public client ID only — no embedded secrets |
| Fork maintainers stealing user tokens | Short-lived access tokens (1 hour), device-specific, revocable |
| Users abusing hosted service infrastructure | Rate limits, quota enforcement, account verification, abuse detection |
| Malware distribution via modified builds | Code signing, release verification, encourage official builds |
| Data exfiltration via modified endpoints | Self-hosted users control their own data; hosted service has audit logs |

### Authentication Flow

**OAuth 2.0 with Device Authorization (RFC 8628)**

```
┌─────────────┐     1. Request Device Code      ┌─────────────────┐
│             │ ───────────────────────────────►│                 │
│  ApexShot   │                                 │  auth.apexshot  │
│   Client    │◄─────────────────────────────── │      .com       │
│             │   device_code, user_code,       │                 │
│             │   verification_uri              │                 │
│             │                                 │                 │
│             │     2. Open browser to          │                 │
│             │        verification_uri         │                 │
│             │ ───────────────────────────────►│                 │
│             │                                 │                 │
│             │     3. User enters user_code    │                 │
│             │        and authorizes           │                 │
│             │                                 │                 │
│             │     4. Poll for token           │                 │
│             │ ───────────────────────────────►│                 │
│             │                                 │                 │
│             │◄─────────────────────────────── │                 │
│             │   access_token, refresh_token   │                 │
└─────────────┘                                 └─────────────────┘
```

**Token Properties:**
- `access_token`: Short-lived (1 hour), used for API calls
- `refresh_token`: Long-lived (30 days), used to obtain new access tokens
- `device_id`: Unique identifier bound to token (prevents token sharing)
- `scope`: `upload`, `delete`, `list` — granular permissions

### Token Storage (Client-Side)

Store tokens securely using platform-specific secret storage:

| Platform | Storage |
|----------|---------|
| Linux | libsecret (GNOME Keyring) / kwallet (KDE) |
| macOS | Keychain Services |
| Windows | Credential Manager |

**Never store tokens in plain text files.**

---

## API Design

### Base URLs

```
Production:     https://api.apexshot.com/v1
Self-hosted:    https://<user-domain>/api/v1
```

### Endpoints

#### Authentication

```
POST /auth/device
  Request:  { client_id: string }
  Response: { device_code, user_code, verification_uri, expires_in, interval }

POST /auth/token
  Request:  { device_code: string, grant_type: "urn:ietf:params:oauth:grant-type:device_code" }
  Response: { access_token, refresh_token, token_type: "Bearer", expires_in, device_id }

POST /auth/refresh
  Request:  { refresh_token: string }
  Response: { access_token, token_type: "Bearer", expires_in }

POST /auth/revoke
  Headers:  Authorization: Bearer <token>
  Request:  { token: string, token_type_hint: "refresh_token" | "access_token" }
  Response: { success: true }
```

#### Uploads

```
POST /uploads
  Headers:  Authorization: Bearer <token>
           X-Device-ID: <device_id>
  Request:  multipart/form-data
            - file: binary
            - filename: string (optional)
            - expires_in: number (days, optional, default: 7 for free tier)
            - password: string (optional, Pro+ only)
            - visibility: "public" | "unlisted" | "private" (Pro+ only)
  Response: {
    id: string,
    url: string,
    delete_url: string,
    thumbnail_url: string,
    expires_at: string (ISO 8601)
  }

GET /uploads
  Headers:  Authorization: Bearer <token>
  Query:    page, limit, sort, order
  Response: { uploads: [...], pagination: { total, page, limit } }

DELETE /uploads/:id
  Headers:  Authorization: Bearer <token>
           X-Device-ID: <device_id>
  Response: { success: true }

GET /uploads/:id
  Headers:  Authorization: Bearer <token> (if private)
  Query:    password (if password-protected)
  Response: Redirect to file or 404
```

#### Account

```
GET /account
  Headers:  Authorization: Bearer <token>
  Response: {
    email: string,
    tier: "free" | "pro" | "team",
    usage: { uploads: number, storage_bytes: number },
    limits: { max_uploads: number, max_storage_bytes: number, retention_days: number }
  }

GET /account/devices
  Headers:  Authorization: Bearer <token>
  Response: { devices: [{ id, name, last_active, created_at }] }

DELETE /account/devices/:id
  Headers:  Authorization: Bearer <token>
  Response: { success: true }
```

#### Subscription (Stripe Integration)

```
POST /subscription/checkout
  Headers:  Authorization: Bearer <token>
  Request:  { tier: "pro" | "team", billing_period: "monthly" | "yearly" }
  Response: { checkout_url: string }

POST /subscription/portal
  Headers:  Authorization: Bearer <token>
  Response: { portal_url: string }

POST /webhooks/stripe
  Headers:  Stripe-Signature: <signature>
  Request:  Stripe webhook payload
  Response: { received: true }
```

### Rate Limits

| Endpoint | Free | Pro | Team |
|----------|------|-----|------|
| POST /uploads | 20/day | Unlimited | Unlimited |
| GET /uploads | 60/min | 120/min | 300/min |
| Auth endpoints | 10/min | 10/min | 10/min |

**Headers:**
```
X-RateLimit-Limit: 60
X-RateLimit-Remaining: 45
X-RateLimit-Reset: 1712923200
```

---

## Pricing & Tiers

| Feature | Free | Pro ($5/mo) | Team ($15/user/mo) |
|---------|------|-------------|---------------------|
| **Uploads/month** | 20 | Unlimited | Unlimited |
| **Storage** | 100 MB | 10 GB | 50 GB per user |
| **Retention** | 7 days | 90 days | Unlimited |
| **Max file size** | 5 MB | 25 MB | 50 MB |
| **Password protection** | ❌ | ✅ | ✅ |
| **Custom domains** | ❌ | ✅ | ✅ |
| **Private uploads** | ❌ | ✅ | ✅ |
| **Shared workspace** | ❌ | ❌ | ✅ |
| **SSO (SAML/OIDC)** | ❌ | ❌ | ✅ |
| **Audit logs** | ❌ | ❌ | ✅ |
| **Priority support** | ❌ | ❌ | ✅ |

### Billing Integration

- **Stripe Checkout** for payment processing
- **Stripe Customer Portal** for self-service billing management
- **Webhooks** to sync subscription status with user accounts
- **Yearly discount:** 2 months free (pay for 10 months)

---

## Self-Hosted Backend

### Reference Implementation

Provide a Docker-ready reference server that users can deploy:

```yaml
# docker-compose.yml
version: '3.8'
services:
  apexshot-cloud:
    image: apexshot/cloud-server:latest
    ports:
      - "3000:3000"
    environment:
      DATABASE_URL: postgres://user:pass@db:5432/apexshot
      STORAGE_BACKEND: s3 | local | gcs
      S3_ENDPOINT: https://s3.amazonaws.com
      S3_BUCKET: my-bucket
      S3_ACCESS_KEY: xxx
      S3_SECRET_KEY: xxx
    depends_on:
      - db

  db:
    image: postgres:15
    volumes:
      - postgres_data:/var/lib/postgresql/data
    environment:
      POSTGRES_DB: apexshot
      POSTGRES_USER: user
      POSTGRES_PASSWORD: pass
```

### Configuration in Client

```json
// ~/.config/apexshot/cloud.json
{
  "backend": "apexshot" | "custom",
  "custom_endpoint": "https://my-server.example.com/api/v1",
  "custom_oauth_endpoint": "https://my-server.example.com/oauth"
}
```

### Self-Hosted vs Hosted Feature Matrix

All features available in self-hosted — no artificial limits. Users own their data.

---

## Client Integration

### Plugin Architecture

```rust
// src/cloud/mod.rs
pub trait CloudProvider: Send + Sync {
    /// Unique identifier for this provider
    fn id(&self) -> &str;

    /// Display name shown in UI
    fn name(&self) -> &str;

    /// Check if provider is configured
    fn is_configured(&self) -> bool;

    /// Initiate OAuth flow
    fn authenticate(&self) -> Result<AuthResult, CloudError>;

    /// Refresh access token
    fn refresh_token(&self) -> Result<(), CloudError>;

    /// Upload file
    fn upload(&self, request: UploadRequest) -> Result<UploadResult, CloudError>;

    /// List uploads
    fn list(&self, options: ListOptions) -> Result<Vec<UploadInfo>, CloudError>;

    /// Delete upload
    fn delete(&self, id: &str) -> Result<(), CloudError>;

    /// Get account info
    fn account(&self) -> Result<AccountInfo, CloudError>;

    /// Sign out (revoke tokens)
    fn sign_out(&self) -> Result<(), CloudError>;
}

pub struct UploadRequest {
    pub file_path: PathBuf,
    pub filename: Option<String>,
    pub expires_in: Option<u32>,      // days
    pub password: Option<String>,
    pub visibility: Visibility,
}

pub struct UploadResult {
    pub id: String,
    pub url: String,
    pub delete_url: String,
    pub thumbnail_url: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
}
```

### Built-in Providers

```rust
// src/cloud/providers/apexshot.rs
pub struct ApexShotCloud {
    config: ApexShotConfig,
    token_store: Arc<dyn TokenStore>,
}

// src/cloud/providers/custom.rs
pub struct CustomProvider {
    endpoint: String,
    oauth_endpoint: String,
    token_store: Arc<dyn TokenStore>,
}

// src/cloud/providers/s3.rs (direct S3, no OAuth)
pub struct S3Direct {
    bucket: String,
    region: String,
    credentials: S3Credentials,
}
```

### UI Integration Points

**Editor Toolbar:**
- Cloud upload icon (existing `cloud-upload.svg`)
- Dropdown menu: "Upload to ApexShot Cloud" | "Upload to [Custom]" | "Configure..."

**Settings Window:**
- New "Cloud" tab
- Provider selection dropdown
- Account status + usage display
- Sign in/out button
- "Configure custom provider" link

**Upload Progress:**
- Progress bar in editor status area
- Cancel button
- On success: Show shareable link with copy button

---

## Backend Architecture

### Tech Stack Recommendation

| Layer | Technology |
|-------|------------|
| **Runtime** | Rust (axum) or Go (fiber) — high performance, low resource usage |
| **Database** | PostgreSQL |
| **Storage** | S3-compatible (AWS S3, Cloudflare R2, self-hosted MinIO) |
| **Auth** | OAuth 2.0 server (custom or Auth.js) |
| **Payments** | Stripe |
| **CDN** | Cloudflare (for public uploads) |
| **Queue** | Redis (for thumbnail generation, cleanup jobs) |

### Database Schema

```sql
CREATE TABLE users (
    id UUID PRIMARY KEY,
    email VARCHAR(255) UNIQUE NOT NULL,
    password_hash VARCHAR(255),
    tier VARCHAR(20) DEFAULT 'free',
    stripe_customer_id VARCHAR(255),
    created_at TIMESTAMP DEFAULT NOW()
);

CREATE TABLE devices (
    id UUID PRIMARY KEY,
    user_id UUID REFERENCES users(id) ON DELETE CASCADE,
    name VARCHAR(100),
    user_agent VARCHAR(255),
    last_active TIMESTAMP,
    created_at TIMESTAMP DEFAULT NOW()
);

CREATE TABLE tokens (
    id UUID PRIMARY KEY,
    device_id UUID REFERENCES devices(id) ON DELETE CASCADE,
    access_token_hash VARCHAR(255) NOT NULL,
    refresh_token_hash VARCHAR(255) NOT NULL,
    scope VARCHAR(100),
    expires_at TIMESTAMP,
    created_at TIMESTAMP DEFAULT NOW()
);

CREATE TABLE uploads (
    id UUID PRIMARY KEY,
    user_id UUID REFERENCES users(id) ON DELETE CASCADE,
    filename VARCHAR(255),
    storage_key VARCHAR(500) NOT NULL,
    size_bytes BIGINT,
    content_type VARCHAR(100),
    visibility VARCHAR(20) DEFAULT 'public',
    password_hash VARCHAR(255),
    expires_at TIMESTAMP,
    created_at TIMESTAMP DEFAULT NOW()
);

CREATE TABLE usage (
    user_id UUID REFERENCES users(id) ON DELETE CASCADE,
    period_start DATE,
    uploads_count INT DEFAULT 0,
    storage_bytes BIGINT DEFAULT 0,
    PRIMARY KEY (user_id, period_start)
);
```

### Background Jobs

- **Thumbnail generation** — Create preview images for uploads
- **Retention cleanup** — Delete expired uploads
- **Usage aggregation** — Calculate monthly usage for billing
- **Abuse detection** — Flag unusual upload patterns

---

## Abuse Prevention

### Detection Rules

| Pattern | Action |
|---------|--------|
| > 100 uploads/hour from single account | Rate limit + manual review |
| File type mismatch (extension ≠ actual type) | Reject upload |
| File size > tier limit | Reject + notify |
| Multiple failed auth attempts | Temporarily block IP |
| DMCA takedown notice | Remove content + warn user |

### Content Moderation

- **Optional:** Integrate with AWS Rekognition or similar for NSFW/malware scanning
- **User reporting:** Allow anyone to report abuse via `report_url`
- **Admin dashboard:** Review flagged content

---

## Implementation Phases

### Phase 1: Foundation
- [ ] Plugin architecture in client
- [ ] OAuth flow implementation
- [ ] Basic upload API
- [ ] Token storage (libsecret/keychain)

### Phase 2: Hosted Service
- [ ] Backend API server
- [ ] Stripe integration
- [ ] Account management UI (web)
- [ ] Free tier enforcement

### Phase 3: Pro Features
- [ ] Password protection
- [ ] Private uploads
- [ ] Custom domains
- [ ] Extended retention

### Phase 4: Team Features
- [ ] Shared workspaces
- [ ] Team management
- [ ] SSO integration
- [ ] Audit logs

### Phase 5: Self-Hosted
- [ ] Reference server Docker image
- [ ] Documentation for deployment
- [ ] Migration guide for existing users

---

## Open Questions

1. **CDN choice:** Cloudflare vs Fastly vs self-hosted?
2. **Image processing:** Generate thumbnails server-side or client-side?
3. **Analytics:** Track upload views? Privacy implications?
4. **Data residency:** Offer region-specific storage for compliance?
5. **API versioning:** URL-based (`/v1/`, `/v2/`) or header-based?

---

## Related Documents

- [API Specification](./cloud-api-spec.md) — OpenAPI/Swagger spec (to be created)
- [Security Audit Checklist](./cloud-security-audit.md) — Pre-launch security review (to be created)
- [Self-Hosted Deployment Guide](./cloud-self-hosted.md) — Step-by-step for users (to be created)
