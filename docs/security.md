# Web API Security Guidance

## No Built-in Authentication

This service does **not** implement any built-in authentication or authorization. All API and SSE endpoints are open to
anyone who can reach the server.

## Critical Warning

**Do NOT expose this service to the public internet!**

Anyone with network access to the server can queue downloads, view status, and access all features. There are no
passwords, tokens, or user accounts.

## Recommended Deployment

- Only run the server on trusted networks (e.g., LAN, VPN, Tailscale, or localhost).
- Use network-level controls (firewalls, VPNs, allow-lists) to restrict access to authorized users/devices.
- If you ever need to expose the service more broadly, add authentication (see below).

## Future-Proofing

The codebase can be refactored later to support authentication (e.g., static token, JWT, OAuth) if requirements change.

---
**Summary:**
This service is designed for private/internal use only. Always restrict access at the network level. For public or
multi-user deployments, add authentication before exposing the service.
