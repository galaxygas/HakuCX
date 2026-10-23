# Contributing to HakuCX

Thanks for helping. HakuCX welcomes contributions from people and from AI agents working on someone's behalf.

## Before you start

- Read [AGENTS.md](AGENTS.md). Its rules apply to everyone.
- For anything larger than a bug fix, open an issue first so the approach can be agreed before you spend time on it.
- Security problems go to the process in [SECURITY.md](SECURITY.md), not to public issues.

## Workflow

1. Fork and create a branch from `main`.
2. Make focused commits. One logical change per pull request.
3. Run the checks:
   ```bash
   cargo fmt --all
   ```
   ```bash
   cargo clippy --workspace -- -D warnings
   ```
   ```bash
   cargo test --workspace
   ```
4. Open a pull request describing what changed, why, and how you verified it. Screenshots for UI changes.
5. CI must pass. A maintainer reviews every change; code owners are listed in `.github/CODEOWNERS`.

## Licensing

HakuCX is licensed under the [MPL-2.0](LICENSE). By contributing you agree that your contribution is licensed under the MPL-2.0. Every new source file starts with the MPL-2.0 header:

```
/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */
```

Third-party code must have a compatible license and keep its notices. New dependencies must pass `cargo deny check`.

## AI-assisted contributions

AI-written code is welcome on the same terms as any other code: you are responsible for it, it must be tested, and the PR must say how it was verified. Do not submit generated code you have not run.
