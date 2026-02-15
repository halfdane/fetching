# Web API Authentication Security

## Approach: Static Token Authentication (X-Auth-Token)

### What
- All API and SSE endpoints require an `X-Auth-Token` HTTP header.
- The token value is set via an environment variable at server startup.
- Requests without the correct token receive a 401 Unauthorized response.

### Why
- **Simplicity:** No need for user accounts, session management, or token rotation.
- **Security:** As strong as the secrecy of the token; suitable for single-user/admin or trusted LAN environments.
- **Automation:** Easy to use with scripts, curl, or browser extensions.
- **Best Practice:** Common for internal tools, admin panels, and single-user web UIs where public access is not required.

### When to Use
- Single-user or admin-only web interfaces.
- Trusted local networks or VPNs.
- No need for user logins or public access.

### When Not to Use
- Multi-user, public, or untrusted environments.
- When you need user tracking, session expiry, or token revocation.
- In those cases, consider JWT, OAuth, or session-based authentication.

### Future-Proofing
- The codebase can be refactored later to support dynamic/session tokens if requirements change.

---
**Summary:**
A static token is simple, robust, and secure for private/internal use. For public or multi-user deployments, upgrade to a more advanced authentication scheme.
