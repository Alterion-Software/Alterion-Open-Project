# Security Policy

## Supported Versions

| Version | Supported |
| ------- | --------- |
| 1.0.x   | Yes (pre-release, API and file format may change) |
| 0.1.x   | No, upgrade to 1.0 |

## Reporting a Vulnerability

If you discover a security vulnerability in Alterion Open Project, **please do not open a public issue or merge request.**

Report it privately by emailing:

**chaceberry686@gmail.com**

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

The app also talks to a network and holds a credential, so in scope as well:

- Recovering a token from a machine other than the one it was issued to. The
  binding is meant to make a copied token store useless elsewhere.
- A token, or any part of one, reaching a log, a crash report, a config file,
  an exported plan or the recovery snapshots.
- Anything in the authorization code exchange: a code or verifier accepted from
  a request that did not originate it, a redirect honoured that should not have
  been, a state or PKCE check that can be skipped.
- On the AOP Collaborate server: reading or writing a plan without being a
  member of it, a token accepted from an issuer other than the configured one,
  or a push accepted against a cursor that should have been refused. A refused
  push must write nothing.
- Plan content from another user reaching a client that is not a member,
  including over the live websocket.

## Out of scope

- Incorrect scheduling results. That is a bug; open an issue.
- Wrong values read from a well-formed `.mpp`. Also a bug; see the parser.
- The webview's own vulnerabilities. Report those to WebKitGTK; we will update
  the dependency.
- Vulnerabilities in an identity provider you host. AOP Collaborate trusts the
  issuer it is configured with, by design. Report those to whoever wrote it.
- A user who can already run code as you reading your token. Nothing stored on
  a machine survives an attacker who is already on it as you; the binding is
  against a copied file, not against local code execution.
