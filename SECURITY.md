# Security Policy

We take the security of OpenTraderWorld seriously. Because it is self-hosted and
handles your personal financial data, we appreciate reports that help keep it safe.

## Reporting a vulnerability

**Please do not open a public issue for security vulnerabilities.**

Instead, report it privately by email to:

**security@opentraderworld.com**

Include as much detail as you can:

- A description of the vulnerability and its impact
- Steps to reproduce (proof-of-concept, affected endpoint/module, request/response)
- The version or commit you tested against
- Your environment (network mode, deployment setup) if relevant

We will acknowledge your report, keep you updated on our progress, and credit you
once a fix is released (unless you prefer to remain anonymous).

## Scope

OpenTraderWorld is self-hosted and localhost-only by default. Reports are most
valuable when they concern:

- Authentication/authorization bypass (session, MCP tokens, module permissions)
- Injection (SQL, command, path traversal), SSRF, or remote code execution
- Exposure of secrets, credentials, or private data
- Vulnerabilities exploitable in the `lan`, `lan_https`, or `web` network modes

Please test only against your own instance. Do not attempt to access data or
systems you do not own.

## Supported versions

OpenTraderWorld is pre-1.0 and moving quickly. Security fixes are applied to the
latest release; please upgrade before reporting.
