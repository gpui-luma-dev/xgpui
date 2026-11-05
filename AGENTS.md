# Repository instructions

## Git approval

- Do not merge pull requests without the user's explicit approval.
- Do not push directly to `main` without the user's explicit approval.
- Requests to publish, deploy, or finish work do not by themselves authorize
  merging a pull request or pushing directly to `main`. Ask for explicit
  approval for the specific action before performing it.

## Core GPUI public API review

- Track every public API addition or change in non-extension GPUI crates.
- For each, document its purpose, source/serialization compatibility impact,
  migration requirements, and whether it could live in an extension instead.
- Complete authorized implementation and verification, then present the concrete
  API changes individually for user review before finalizing or releasing them.
- Implementation approval does not imply approval of the final public API.
