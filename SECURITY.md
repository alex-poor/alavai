# Security Policy

alavai controls a VPN daemon, so we take security reports seriously.

## Reporting a vulnerability

**Please do not open a public issue for security vulnerabilities.**

Instead, report privately using GitHub's
[private vulnerability reporting](https://github.com/alex-poor/alavai/security/advisories/new)
(the **Security → Report a vulnerability** button on the repository). If that is
unavailable, contact the maintainer via [@alex-poor](https://github.com/alex-poor).

Please include:

- a description of the issue and its impact,
- steps to reproduce, and
- affected version / commit.

We'll acknowledge your report as soon as we can and keep you updated on a fix.

## Scope

alavai talks to the local `tailscaled` daemon over its unix socket and requires
the user to be the Tailscale operator. Issues that involve privilege escalation,
unintended exposure of tailnet data, or unsafe handling of daemon responses are
in scope. Issues in Tailscale itself should be reported to
[Tailscale](https://tailscale.com/security).
