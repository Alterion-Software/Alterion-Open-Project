# Security Policy

## Supported Versions

| Version | Supported |
| ------- | --------- |
| 0.1.x   | Yes (pre-release — API and file format may change) |

## Reporting a Vulnerability

If you discover a security vulnerability in Alterion Open Project, **please do not open a public issue or merge request.**

Report it privately by emailing:

**security@alterion.dpdns.org**

Please include:
- A description of the vulnerability
- Steps to reproduce or a proof-of-concept
- The version(s) affected
- Any suggested mitigation

You will get an acknowledgement within 72 hours and an assessment within 7 days.

## Scope

The application opens files it did not write. A plan can arrive by email, from a
shared drive, or from a colleague using Microsoft Project, and every length,
offset and count inside one is attacker-controlled. In scope:

- A panic, hang or out-of-bounds read reachable from a malformed `.aprj`,
  `.mpp` or MSPDI `.xml`
- Unbounded allocation driven by a length field in a file
- Anything that escapes the file being opened: a path written outside the
  location the user chose, or a command run as a result of opening a plan
- Script injection through the print or export path, where plan text becomes
  part of an HTML document

## Out of scope

- Incorrect scheduling results. That is a bug; open an issue.
- Wrong values read from a well-formed `.mpp`. Also a bug; see the parser.
- The webview's own vulnerabilities. Report those to WebKitGTK; we will update
  the dependency.
